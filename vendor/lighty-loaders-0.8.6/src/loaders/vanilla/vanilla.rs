use super::vanilla_metadata::{PistonMetaManifest, Rule, VanillaAssetFile, VanillaMetaData};
use crate::types::version_metadata::{
    Arguments, Asset, AssetIndex, AssetsFile, Client, JavaVersion, Library, MainClass, Native,
    Version, VersionMetaData,
};
use crate::types::VersionInfo;
use crate::utils::error::QueryError;
use crate::utils::manifest::ManifestRepository;
use crate::utils::query::Query;
use async_trait::async_trait;
use lighty_core::hosts::HTTP_CLIENT as CLIENT;
use lighty_core::system::{ARCHITECTURE, OS};
use lighty_core::{mkdir, verify_file_sha1};
use once_cell::sync::Lazy;

pub type Result<T> = std::result::Result<T, QueryError>;

const CLIENT_NAME: &str = "client";

pub static VANILLA: Lazy<ManifestRepository<VanillaQuery>> =
    Lazy::new(|| ManifestRepository::new());

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VanillaQuery {
    JavaVersion,
    MainClass,
    Libraries,
    Natives,
    AssetsIndex,
    Assets,
    Client,
    Arguments,
    VanillaBuilder,
}

#[async_trait]
impl Query for VanillaQuery {
    type Query = VanillaQuery;
    type Data = VersionMetaData;
    type Raw = VanillaMetaData;

    fn name() -> &'static str {
        "vanilla"
    }
    //OPTIONNEL FAIT UNE IMPLEMENTATION POUR LE TTL SUR LE CACHE
    // fn cache_ttl() -> Duration {
    //     Duration::from_secs(10 * 60)
    // }
    //
    // fn cache_ttl_for_query(query: &Self::Query) -> Duration {
    //     match query {
    //         VanillaQuery::Libraries => Duration::from_secs(10 * 60),
    //         VanillaQuery::Assets => Duration::from_secs(5 * 60),
    //         VanillaQuery::Client => Duration::from_secs(3600),
    //         _ => Duration::from_secs(1800),
    //     }
    // }

    async fn fetch_full_data<V: VersionInfo>(version: &V) -> Result<VanillaMetaData> {
        let manifest_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        lighty_core::trace_info!("Fetching manifest from {}", manifest_url);

        let manifest: PistonMetaManifest = CLIENT.get(manifest_url).send().await?.json().await?;

        let version_info = manifest
            .versions
            .iter()
            .find(|v| v.id == version.minecraft_version())
            .ok_or_else(|| QueryError::VersionNotFound {
                version: version.minecraft_version().to_string(),
            })?;

        let vanilla_metadata: VanillaMetaData =
            CLIENT.get(&version_info.url).send().await?.json().await?;

        Ok(vanilla_metadata)
    }

    async fn extract<V: VersionInfo>(
        version: &V,
        query: &Self::Query,
        full_data: &Self::Raw,
    ) -> Result<Self::Data> {
        let result = match query {
            VanillaQuery::JavaVersion => {
                VersionMetaData::JavaVersion(extract_java_version(full_data))
            }
            VanillaQuery::MainClass => VersionMetaData::MainClass(extract_main_class(full_data)),
            VanillaQuery::Libraries => VersionMetaData::Libraries(extract_libraries(full_data)),
            VanillaQuery::Natives => VersionMetaData::Natives(extract_natives(full_data)?),
            VanillaQuery::AssetsIndex => {
                VersionMetaData::AssetsIndex(extract_assets_index(full_data))
            }
            VanillaQuery::Assets => {
                VersionMetaData::Assets(extract_assets(version, full_data).await?)
            }
            VanillaQuery::Client => VersionMetaData::Client(extract_client(version, full_data)?),
            VanillaQuery::Arguments => VersionMetaData::Arguments(extract_arguments(full_data)),
            VanillaQuery::VanillaBuilder => {
                VersionMetaData::Version(Self::version_builder(version, full_data).await?)
            }
        };

        Ok(result)
    }

    async fn version_builder<V: VersionInfo>(
        version: &V,
        full_data: &VanillaMetaData,
    ) -> Result<Version> {
        Ok(Version {
            main_class: extract_main_class(full_data),
            java_version: extract_java_version(full_data),
            arguments: extract_arguments(full_data),
            libraries: extract_libraries(full_data),
            mods: None,
            natives: Some(extract_natives(full_data)?),
            client: extract_client(version, full_data).ok(),
            assets_index: Some(extract_assets_index(full_data)),
            assets: Some(extract_assets(version, full_data).await?),
            //assets:None,
        })
    }
}

/// --------- Libraries ----------
fn extract_libraries(full_data: &VanillaMetaData) -> Vec<Library> {
    full_data
        .libraries
        .iter()
        .filter(|lib| !lib.name.contains(":natives-") && lib.downloads.classifiers.is_none())
        .filter_map(|lib| {
            lib.downloads.artifact.as_ref().map(|a| Library {
                name: lib.name.clone(),
                url: Some(a.url.clone()),
                path: Some(a.path.clone()),
                sha1: Some(a.sha1.clone()),
                size: Some(a.size),
            })
        })
        .collect()
}

/// --------- Natives ----------
fn extract_natives(full_data: &VanillaMetaData) -> Result<Vec<Native>> {
    let os_name = OS.get_vanilla_os()
        .map_err(|_| QueryError::Conversion {
            message: format!("Unsupported operating system. Only Windows, Linux, and macOS are supported for native extraction. Detected OS: {:?}", std::env::consts::OS)
        })?;

    let arch_suffix = ARCHITECTURE.get_vanilla_arch()
        .map_err(|_| QueryError::Conversion {
            message: format!("Unsupported architecture. Only x86, x64, ARM, and ARM64 are supported. Detected architecture: {:?}", std::env::consts::ARCH)
        })?;

    let arch_bits = ARCHITECTURE
        .get_arch_bits()
        .map_err(|_| QueryError::Conversion {
            message: format!(
                "Unable to determine architecture bits (32 or 64). Detected architecture: {:?}",
                std::env::consts::ARCH
            ),
        })?;

    let natives = full_data
        .libraries
        .iter()
        .filter_map(|lib| {
            // Cas 1: Nouveau format
            if lib.name.contains(":natives-") {
                let exact_pattern = format!(":natives-{}{}", os_name, arch_suffix);

                if lib.name.ends_with(&exact_pattern) {
                    if let Some(rules) = &lib.rules {
                        if !should_apply_rules(rules, os_name) {
                            return None;
                        }
                    }

                    return lib.downloads.artifact.as_ref().map(|a| Native {
                        name: lib.name.clone(),
                        url: Some(a.url.clone()),
                        path: Some(a.path.clone()),
                        sha1: Some(a.sha1.clone()),
                        size: Some(a.size),
                    });
                }
            }

            // Cas 2: Ancien format
            if let Some(natives_map) = &lib.natives {
                if let Some(classifiers) = &lib.downloads.classifiers {
                    if let Some(rules) = &lib.rules {
                        if !should_apply_rules(rules, os_name) {
                            return None;
                        }
                    }

                    if let Some(classifier_pattern) = natives_map.get(os_name) {
                        let classifier_name = classifier_pattern.replace("${arch}", arch_bits);

                        if let Some(artifact) = classifiers.get(&classifier_name) {
                            return Some(Native {
                                name: lib.name.clone(),
                                url: Some(artifact.url.clone()),
                                path: Some(artifact.path.clone()),
                                sha1: Some(artifact.sha1.clone()),
                                size: Some(artifact.size),
                            });
                        }
                    }
                }
            }

            None
        })
        .collect();

    Ok(natives)
}

/// Vérifie si les rules permettent d'utiliser cette bibliothèque sur l'OS actuel
fn should_apply_rules(rules: &[Rule], os_name: &str) -> bool {
    let mut allowed = false;

    for rule in rules {
        let matches_os = rule
            .os
            .as_ref()
            .map_or(true, |os| os.name.as_deref() == Some(os_name));

        if matches_os {
            allowed = rule.action == "allow";
        }
    }

    allowed
}

/// --------- Java Version ----------
fn extract_java_version(full_data: &VanillaMetaData) -> JavaVersion {
    full_data
        .java_version
        .as_ref()
        .map(|v| JavaVersion {
            major_version: v.major_version as u8,
        })
        .unwrap_or_else(|| {
            // For very old Minecraft versions (<1.17), java_version is not specified
            // Default to Java 8 which is compatible with legacy versions
            JavaVersion { major_version: 8 }
        })
}

/// --------- Main Class ----------
fn extract_main_class(full_data: &VanillaMetaData) -> MainClass {
    MainClass {
        main_class: full_data.main_class.clone(),
    }
}

/// --------- Assets ----------
fn extract_assets_index(full_data: &VanillaMetaData) -> AssetIndex {
    AssetIndex {
        id: full_data.asset_index.id.clone(),
        url: full_data.asset_index.url.clone(),
        sha1: full_data.asset_index.sha1.clone(),
        size: full_data.asset_index.size,
        total_size: full_data.asset_index.total_size,
    }
}

async fn extract_assets<V: VersionInfo>(
    version: &V,
    full_data: &VanillaMetaData,
) -> Result<AssetsFile> {
    let asset_index = &full_data.asset_index;

    // Créer le dossier assets/indexes
    let indexes_dir = version.game_dirs().join("assets").join("indexes");
    mkdir!(indexes_dir);

    // Chemin du fichier index (ex: assets/indexes/1.7.10.json ou 26.json)
    let index_file_path = indexes_dir.join(format!("{}.json", asset_index.id));

    // Vérifier si le fichier existe et est valide
    let needs_download = if index_file_path.exists() {
        match verify_file_sha1(&index_file_path, &asset_index.sha1).await {
            Ok(true) => {
                lighty_core::trace_info!(
                    "[Assets] Index {} already cached and valid",
                    asset_index.id
                );
                false
            }
            _ => {
                lighty_core::trace_warn!(
                    "[Assets] Index {} SHA1 mismatch, re-downloading",
                    asset_index.id
                );
                let _ = tokio::fs::remove_file(&index_file_path).await;
                true
            }
        }
    } else {
        true
    };

    // Télécharger si nécessaire
    if needs_download {
        lighty_core::trace_info!(
            "[Assets] Downloading index {} from {}",
            asset_index.id,
            asset_index.url
        );

        let response = CLIENT.get(&asset_index.url).send().await?;

        if !response.status().is_success() {
            return Err(QueryError::Conversion {
                message: format!("Failed to download asset index: HTTP {}", response.status()),
            });
        }

        let content = response.bytes().await?;

        tokio::fs::write(&index_file_path, &content).await?;

        match verify_file_sha1(&index_file_path, &asset_index.sha1).await {
            Ok(true) => {
                lighty_core::trace_info!(
                    "[Assets] Index {} downloaded and verified",
                    asset_index.id
                );
            }
            _ => {
                let _ = tokio::fs::remove_file(&index_file_path).await;
                return Err(QueryError::Conversion {
                    message: format!("Downloaded asset index failed SHA1 verification"),
                });
            }
        }
    }

    let index_content = tokio::fs::read_to_string(&index_file_path).await?;
    let vanilla_assets: VanillaAssetFile = serde_json::from_str(&index_content)?;

    // Construire l'AssetsFile avec les URLs
    let objects = vanilla_assets
        .objects
        .into_iter()
        .map(|(k, v)| {
            let url = Some(format!(
                "https://resources.download.minecraft.net/{}/{}",
                &v.hash[0..2],
                v.hash
            ));
            (
                k,
                Asset {
                    hash: v.hash,
                    size: v.size,
                    url,
                },
            )
        })
        .collect();

    Ok(AssetsFile { objects })
}

fn extract_client<V: VersionInfo>(version: &V, full_data: &VanillaMetaData) -> Result<Client> {
    full_data
        .downloads
        .client
        .as_ref()
        .map(|client| Client {
            name: CLIENT_NAME.into(),
            url: Some(client.url.clone()),
            path: Some(format!("{}.jar", version.name())),
            sha1: Some(client.sha1.clone()),
            size: Some(client.size),
        })
        .ok_or_else(|| QueryError::MissingField {
            field: CLIENT_NAME.into(),
        })
}

/// --------- Arguments ----------
fn extract_arguments(full_data: &VanillaMetaData) -> Arguments {
    if let Some(args) = &full_data.arguments {
        Arguments {
            game: args
                .game
                .iter()
                .filter_map(|a| a.as_str().map(String::from))
                .collect(),
            jvm: Some(
                args.jvm
                    .iter()
                    .filter_map(|a| a.as_str().map(String::from))
                    .collect(),
            ),
        }
    } else if let Some(legacy) = &full_data.minecraft_arguments {
        Arguments {
            game: legacy.split_whitespace().map(String::from).collect(),
            jvm: None,
        }
    } else {
        Arguments {
            game: vec![],
            jvm: None,
        }
    }
}
