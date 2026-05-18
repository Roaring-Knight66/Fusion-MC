use crate::loaders::forge::forge_metadata::ForgeMetaData;
use crate::loaders::forge::forge_metadata::ForgeVersionMeta;
use log::error;
use zip::ZipArchive;
use std::fs::File;
use std::io::{Read, Cursor};
use std::path::PathBuf;
use lighty_core::mkdir;
use lighty_core::download::download_file_untracked;
use lighty_version::version_metadata::{JavaVersion, Library, MainClass, Arguments, VersionBuilder, VersionMetaData};
use crate::version::Version;
use crate::loaders::utils::{error::QueryError, query::Query, manifest::ManifestRepository};
use crate::loaders::vanilla::{vanilla::VanillaQuery, vanilla_metadata::VanillaMetaData};
use once_cell::sync::Lazy;
use async_trait::async_trait;
use lighty_core::hosts::HTTP_CLIENT as CLIENT;
use sha1::{Sha1, Digest};

pub type Result<T> = std::result::Result<T, QueryError>;

pub static NEOFORGE: Lazy<ManifestRepository<ForgeQuery>> = Lazy::new(|| ManifestRepository::new());

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ForgeQuery {
    ForgeBuilder,
}

#[async_trait]
impl Query for ForgeQuery {
    type Query = ForgeQuery;
    type Data = VersionMetaData;
    type Raw = ForgeMetaData;

    fn name() -> &'static str {
        "neoforge"
    }

    async fn fetch_full_data(version: &Version) -> Result<ForgeMetaData> {
        // Construire l'URL de l'installer
        let installer_url = if is_old_neoforge(version) {
            // Pour les versions anciennes (≤ 1.20.1), utiliser l'ancien format Forge
            let path_version = format!("{}-{}", version.minecraft_version, version.loader_version);
            let file_prefix = format!("forge-{}", version.minecraft_version);
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/forge/{}/{}-{}-installer.jar",
                path_version, file_prefix, version.loader_version
            )
        } else {
            // Pour les nouvelles versions (> 1.20.1), utiliser le nouveau format NeoForge
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
                version.loader_version, version.loader_version
            )
        };

        println!("[NeoForgeLoader] Installer URL: {}", installer_url);

        // Créer le répertoire de profil NeoForge
        let profiles_dir = version.game_dirs.join(".neoforge");
        mkdir!(profiles_dir);

        // Chemin de l'installer local
        let installer_path = profiles_dir.join(format!("neoforge-{}-installer.jar", version.loader_version));

        // Vérifier et télécharger l'installer si nécessaire
        let needs_download = if installer_path.exists() {
            // Vérifier le SHA1 si le fichier existe
            match verify_installer_sha1(&installer_path, &installer_url).await {
                Ok(true) => {
                    println!("[NeoForgeLoader] Installer already exists and SHA1 is valid");
                    false
                }
                Ok(false) => {
                    println!("[NeoForgeLoader] Installer exists but SHA1 mismatch, re-downloading");
                    true
                }
                Err(e) => {
                    println!("[NeoForgeLoader] Could not verify SHA1 ({}), using existing file", e);
                    false
                }
            }
        } else {
            true
        };

        if needs_download {
            println!("[NeoForgeLoader] Downloading installer to: {:?}", installer_path);
            download_file_untracked(&installer_url, &installer_path)
                .await
                .map_err(|e| QueryError::Conversion {
                    message: format!("Failed to download installer: {}", e)
                })?;

            // Vérifier le SHA1 après téléchargement
            if let Ok(valid) = verify_installer_sha1(&installer_path, &installer_url).await {
                if !valid {
                    return Err(QueryError::Conversion {
                        message: "Downloaded installer has invalid SHA1".to_string()
                    });
                }
            }
        }

        // Lire les JSONs directement depuis le JAR sans extraction
        let (install_profile, version_meta) = read_jsons_from_jar(&installer_path).await?;

        println!("[NeoForgeLoader] Successfully loaded NeoForge metadata");

        Ok(install_profile)
    }

    async fn extract(version: &Version, query: &Self::Query, full_data: &ForgeMetaData) -> Result<Self::Data> {
        let result = match query {
            ForgeQuery::ForgeBuilder => {
                VersionMetaData::VersionBuilder(Self::version_builder(version, full_data).await?)
            }
        };
        Ok(result)
    }

    async fn version_builder(version: &Version, full_data: &ForgeMetaData) -> Result<VersionBuilder> {
        // Récupérer les données Vanilla
        let vanilla_data: VanillaMetaData = VanillaQuery::fetch_full_data(version).await?;
        let vanilla_builder: VersionBuilder = VanillaQuery::version_builder(version, &vanilla_data).await?;

        // Lire version.json directement depuis le JAR
        let profiles_dir = version.game_dirs.join(".neoforge");
        let installer_path = profiles_dir.join(format!("neoforge-{}-installer.jar", version.loader_version));
        let (_, version_meta) = read_jsons_from_jar(&installer_path).await?;

        // Construire le builder NeoForge
        let neoforge_builder = VersionBuilder {
            main_class: extract_main_class(&version_meta),
            java_version: JavaVersion { major_version: 8 },
            arguments: extract_arguments(&version_meta),
            libraries: extract_libraries(full_data),
            natives: None,
            client: None,
            assets_index: None,
            assets: None,
        };

        // Merger les deux builders
        Ok(VersionBuilder {
            main_class: merge_main_class(vanilla_builder.main_class, neoforge_builder.main_class),
            java_version: neoforge_builder.java_version,
            arguments: merge_arguments(vanilla_builder.arguments, neoforge_builder.arguments),
            libraries: merge_libraries(vanilla_builder.libraries, neoforge_builder.libraries),
            natives: vanilla_builder.natives.or(neoforge_builder.natives),
            client: vanilla_builder.client.or(neoforge_builder.client),
            assets_index: vanilla_builder.assets_index.or(neoforge_builder.assets_index),
            assets: vanilla_builder.assets.or(neoforge_builder.assets),
        })
    }
}

/// --------- Fonctions de merge ----------

fn merge_main_class(vanilla: MainClass, neoforge: MainClass) -> MainClass {
    if neoforge.main_class.is_empty() {
        vanilla
    } else {
        neoforge
    }
}

fn merge_arguments(vanilla: Arguments, neoforge: Arguments) -> Arguments {
    Arguments {
        game: {
            let mut args = vanilla.game;
            args.extend(neoforge.game);
            args
        },
        jvm: match (vanilla.jvm, neoforge.jvm) {
            (Some(mut v), Some(n)) => {
                v.extend(n);
                Some(v)
            }
            (Some(v), None) => Some(v),
            (None, Some(n)) => Some(n),
            (None, None) => None,
        },
    }
}

fn merge_libraries(mut vanilla_libs: Vec<Library>, neoforge_libs: Vec<Library>) -> Vec<Library> {
    for lib in neoforge_libs {
        if !vanilla_libs.iter().any(|v| v.name == lib.name) {
            vanilla_libs.push(lib);
        }
    }
    vanilla_libs
}

/// --------- Fonctions d'extraction ----------

fn extract_main_class(version_meta: &ForgeVersionMeta) -> MainClass {
    MainClass {
        main_class: version_meta.main_class.clone(),
    }
}

fn extract_arguments(version_meta: &ForgeVersionMeta) -> Arguments {
    Arguments {
        game: version_meta.arguments.game.clone(),
        jvm: Some(version_meta.arguments.jvm.clone()),
    }
}

fn extract_libraries(full_data: &ForgeMetaData) -> Vec<Library> {
    full_data
        .libraries
        .iter()
        .map(|lib| Library {
            name: lib.name.clone(),
            url: Some(lib.downloads.artifact.url.clone()),
            path: Some(lib.downloads.artifact.path.clone()),
            sha1: Some(lib.downloads.artifact.sha1.clone()),
            size: Some(lib.downloads.artifact.size),
        })
        .collect()
}

/// --------- Helpers ----------

fn is_old_neoforge(version: &Version) -> bool {
    version_compare::compare_to(&version.minecraft_version, "1.20.1", version_compare::Cmp::Le)
        .unwrap_or(false)
}

/// Lit les JSONs directement depuis le JAR sans extraction sur disque
async fn read_jsons_from_jar(installer_path: &PathBuf) -> Result<(ForgeMetaData, ForgeVersionMeta)> {
    let path = installer_path.clone();

    tokio::task::spawn_blocking(move || {
        let file = File::open(&path).map_err(|e| QueryError::Conversion {
            message: format!("Failed to open installer JAR: {}", e)
        })?;

        let mut archive = ZipArchive::new(file).map_err(|e| QueryError::Conversion {
            message: format!("Failed to open ZIP archive: {}", e)
        })?;

        // Lire install_profile.json
        let install_profile = {
            let mut file = archive.by_name("install_profile.json").map_err(|_| {
                QueryError::MissingField {
                    field: "install_profile.json in installer JAR".to_string(),
                }
            })?;

            let mut contents = String::new();
            file.read_to_string(&mut contents).map_err(|e| QueryError::Conversion {
                message: format!("Failed to read install_profile.json: {}", e)
            })?;

            serde_json::from_str::<ForgeMetaData>(&contents)?
        };

        // Lire version.json
        let version_meta = {
            let mut file = archive.by_name("version.json").map_err(|_| {
                QueryError::MissingField {
                    field: "version.json in installer JAR".to_string(),
                }
            })?;

            let mut contents = String::new();
            file.read_to_string(&mut contents).map_err(|e| QueryError::Conversion {
                message: format!("Failed to read version.json: {}", e)
            })?;

            serde_json::from_str::<ForgeVersionMeta>(&contents)?
        };

        Ok((install_profile, version_meta))
    })
    .await
    .map_err(|e| QueryError::Conversion {
        message: format!("Failed to spawn blocking task: {}", e)
    })?
}

/// Récupère le SHA1 attendu depuis Maven
async fn fetch_maven_sha1(jar_url: &str) -> Option<String> {
    let sha1_url = format!("{}.sha1", jar_url);

    match CLIENT.get(&sha1_url).send().await {
        Ok(response) if response.status().is_success() => {
            response.text().await.ok().and_then(|text| {
                let sha1 = text.trim().split_whitespace().next()?.to_string();
                (sha1.len() == 40).then_some(sha1)
            })
        }
        _ => None,
    }
}

/// Calcule le SHA1 d'un fichier
fn calculate_file_sha1(path: &PathBuf) -> Result<String> {
    let mut file = File::open(path).map_err(|e| QueryError::Conversion {
        message: format!("Failed to open file for SHA1 calculation: {}", e)
    })?;

    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];

    loop {
        let n = file.read(&mut buffer).map_err(|e| QueryError::Conversion {
            message: format!("Failed to read file for SHA1 calculation: {}", e)
        })?;

        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}


/// Vérifie le SHA1 de l'installer
async fn verify_installer_sha1(installer_path: &PathBuf, installer_url: &str) -> Result<bool> {
    // Récupérer le SHA1 attendu
    let expected_sha1 = fetch_maven_sha1(installer_url)
        .await
        .ok_or_else(|| QueryError::Conversion {
            message: "Failed to fetch SHA1 from Maven".to_string()
        })?;

    // Calculer le SHA1 du fichier local
    let actual_sha1 = calculate_file_sha1(installer_path)?;

    Ok(expected_sha1.eq_ignore_ascii_case(&actual_sha1))
}