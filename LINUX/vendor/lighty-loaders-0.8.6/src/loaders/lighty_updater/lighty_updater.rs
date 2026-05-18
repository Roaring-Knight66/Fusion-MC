use super::lighty_metadata::{LightyMetadata, ServersResponse};
use crate::types::version_metadata::{
    Arguments, Asset, AssetsFile, Client, JavaVersion, Library, MainClass, Mods, Native, Version,
    VersionMetaData,
};
use crate::types::{Loader, VersionInfo};
use crate::utils::{error::QueryError, manifest::ManifestRepository, query::Query};
use async_trait::async_trait;
use lighty_core::hosts::HTTP_CLIENT as CLIENT;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, QueryError>;

/// Version override pour LightyUpdater
///
/// Utilisé pour passer les bonnes valeurs de minecraft_version et loader
/// aux loaders de base (vanilla, fabric, etc.) lors du merge.
///
/// Cette structure est spécifique à LightyUpdater car on a besoin d'override
/// ces valeurs depuis ServerInfo avant de fetcher le loader de base.
#[derive(Debug, Clone)]
struct VersionOverride {
    name: String,
    loader_version: String,
    minecraft_version: String,
    loader: Loader,
    game_dirs: PathBuf,
    java_dirs: PathBuf,
}

impl VersionInfo for VersionOverride {
    type LoaderType = Loader;

    fn name(&self) -> &str {
        &self.name
    }

    fn loader_version(&self) -> &str {
        &self.loader_version
    }

    fn minecraft_version(&self) -> &str {
        &self.minecraft_version
    }

    fn game_dirs(&self) -> &Path {
        &self.game_dirs
    }

    fn java_dirs(&self) -> &Path {
        &self.java_dirs
    }

    fn loader(&self) -> &Self::LoaderType {
        &self.loader
    }
}

pub static LIGHTY_UPDATER: Lazy<ManifestRepository<LightyQuery>> =
    Lazy::new(|| ManifestRepository::new());

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LightyQuery {
    Libraries,
    Arguments,
    MainClass,
    Mods,
    Assets,
    LightyBuilder,
}

#[async_trait]
impl Query for LightyQuery {
    type Query = LightyQuery;
    type Data = VersionMetaData;
    type Raw = LightyMetadata;

    fn name() -> &'static str {
        "lighty_updater"
    }

    async fn fetch_full_data<V: VersionInfo>(version: &V) -> Result<LightyMetadata> {
        lighty_core::trace_debug!("🚀 [LightyUpdater] fetch_full_data START");
        lighty_core::trace_debug!("   version.name() = {}", version.name());
        lighty_core::trace_debug!("   version.loader_version() = {}", version.loader_version());

        // 1. Récupérer les infos du serveur Lighty
        let server_info_url = format!("{}/", version.loader_version());
        lighty_core::trace_debug!(
            "📡 [LightyUpdater] Fetching ServerInfo from: {}",
            server_info_url
        );

        let response = CLIENT.get(&server_info_url).send().await;
        lighty_core::trace_debug!(
            "📡 [LightyUpdater] HTTP Response: {:?}",
            response.as_ref().map(|r| r.status())
        );

        let response = response?;
        let text = response.text().await?;
        lighty_core::trace_debug!("📄 [LightyUpdater] Raw JSON response: {}", text);

        let servers_response: ServersResponse = serde_json::from_str(&text).map_err(|e| {
            lighty_core::trace_error!("❌ [LightyUpdater] JSON parsing failed: {}", e);
            lighty_core::trace_error!("   Expected ServersResponse with 'servers' array");
            QueryError::JsonParsing(e)
        })?;

        // Trouver le serveur correspondant au nom de la version
        let server_info = servers_response
            .find_by_name(version.name())
            .cloned()
            .ok_or_else(|| {
                lighty_core::trace_error!(
                    "❌ [LightyUpdater] Server '{}' not found in servers list",
                    version.name()
                );
                QueryError::VersionNotFound {
                    version: version.name().to_string(),
                }
            })?;

        lighty_core::trace_info!(
            "✅ [LightyUpdater] ServerInfo retrieved: name={}, loader={}, mc_version={}, last_update={}",
            server_info.name(),
            server_info.loader(),
            server_info.minecraft_version(),
            server_info.last_update()
        );

        // 2. Utiliser directement l'URL complète fournie par le serveur
        let metadata_url = server_info.url();
        lighty_core::trace_debug!(
            "📡 [LightyUpdater] Fetching LightyMetadata from: {}",
            metadata_url
        );

        let meta_response = CLIENT.get(metadata_url).send().await;
        lighty_core::trace_debug!(
            "📡 [LightyUpdater] Metadata HTTP Response: {:?}",
            meta_response.as_ref().map(|r| r.status())
        );

        let mut manifest: LightyMetadata = meta_response?.json().await?;

        // 3. Stocker le server_info dans la metadata pour éviter un double fetch
        manifest.server_info = Some(server_info);

        lighty_core::trace_debug!("✅ [LightyUpdater] LightyMetadata retrieved successfully");
        Ok(manifest)
    }

    async fn extract<V: VersionInfo>(
        version: &V,
        query: &Self::Query,
        full_data: &LightyMetadata,
    ) -> Result<Self::Data> {
        let result = match query {
            LightyQuery::Libraries => VersionMetaData::Libraries(extract_libraries(full_data)),
            LightyQuery::Arguments => VersionMetaData::Arguments(extract_arguments(full_data)),
            LightyQuery::MainClass => VersionMetaData::MainClass(extract_main_class(full_data)),
            LightyQuery::Mods => VersionMetaData::Mods(extract_mods(full_data)),
            LightyQuery::Assets => VersionMetaData::Assets(extract_assets(full_data)),
            LightyQuery::LightyBuilder => {
                VersionMetaData::Version(Self::version_builder(version, full_data).await?)
            }
        };
        Ok(result)
    }

    async fn version_builder<V: VersionInfo>(
        version: &V,
        full_data: &LightyMetadata,
    ) -> Result<Version> {
        use super::merge_metadata::merge_metadata;

        lighty_core::trace_debug!("🔧 [LightyUpdater] version_builder START");

        // Récupérer le server_info depuis full_data (déjà fetché par fetch_full_data)
        let server_info = full_data
            .server_info
            .as_ref()
            .ok_or_else(|| QueryError::InvalidMetadata)?;

        lighty_core::trace_debug!(
            "✅ [LightyUpdater] ServerInfo for merge: loader={}, mc_version={}",
            server_info.loader(),
            server_info.minecraft_version()
        );

        // Convertir le loader string en enum
        let loader = match server_info.loader() {
            "vanilla" => Loader::Vanilla,
            "fabric" => Loader::Fabric,
            "quilt" => Loader::Quilt,
            "neoforge" => Loader::NeoForge,
            "forge" => Loader::Forge,
            _ => Loader::LightyUpdater,
        };

        // Créer le VersionOverride avec les vraies valeurs du serveur
        let version_override = VersionOverride {
            name: version.name().to_string(),
            loader_version: version.loader_version().to_string(),
            minecraft_version: server_info.minecraft_version().to_string(),
            loader,
            game_dirs: version.game_dirs().to_path_buf(),
            java_dirs: version.java_dirs().to_path_buf(),
        };

        // 1. Fetch du loader de base avec l'override
        lighty_core::trace_debug!(
            "🔀 [LightyUpdater] Calling merge_metadata with loader={}",
            server_info.loader()
        );
        let mut builder = merge_metadata(&version_override, server_info.loader()).await?;
        lighty_core::trace_debug!("✅ [LightyUpdater] Base loader metadata merged");

        // 2. OVERRIDE avec LightyMetadata (priorité à Lighty!)

        // Client : Si Lighty a un client, on l'utilise
        if !full_data.client.url.is_empty() {
            builder.client = Some(extract_client(full_data));
        }

        // Natives : On MERGE avec ceux du loader
        if !full_data.natives.is_empty() {
            let lighty_natives = extract_natives(full_data);
            builder.natives = Some(merge_natives(
                builder.natives.unwrap_or_default(),
                lighty_natives,
            ));
        }

        // Assets : On MERGE avec ceux du loader
        if !full_data.assets.is_empty() {
            let lighty_assets = extract_assets(full_data);
            builder.assets = Some(merge_assets(
                builder.assets.unwrap_or_else(|| AssetsFile {
                    objects: HashMap::new(),
                }),
                lighty_assets,
            ));
        }

        // Libraries : On MERGE (ajoute les libs de Lighty aux libs du loader)
        if !full_data.libraries.is_empty() {
            let lighty_libs = extract_libraries(full_data);
            builder.libraries = merge_libraries(builder.libraries, lighty_libs);
        }

        // Mods : Toujours de Lighty
        builder.mods = Some(extract_mods(full_data));

        // MainClass : Si Lighty a une mainClass, on l'utilise
        if !full_data.main_class.main_class.is_empty() {
            builder.main_class = extract_main_class(full_data);
        }

        // Arguments : On merge
        builder.arguments = merge_arguments(builder.arguments, extract_arguments(full_data));

        // JavaVersion : Si Lighty spécifie une version, on l'utilise
        if full_data.java_version.major_version > 0 {
            builder.java_version = extract_java_version(full_data);
        }

        lighty_core::trace_info!(
            loader = %server_info.loader(),
            mods_count = builder.mods.as_ref().map(|m| m.len()).unwrap_or(0),
            "Merged Lighty Updater with {} loader",
            server_info.loader()
        );

        Ok(builder)
    }
}

/// Extrait le VersionBuilder depuis VersionMetaData
pub(crate) fn extract_version_builder(data: std::sync::Arc<VersionMetaData>) -> Result<Version> {
    match data.as_ref() {
        VersionMetaData::Version(builder) => Ok(builder.clone()),
        _ => Err(QueryError::InvalidMetadata),
    }
}

fn extract_main_class(full_data: &LightyMetadata) -> MainClass {
    MainClass {
        main_class: full_data.main_class.main_class.clone(),
    }
}

fn extract_java_version(full_data: &LightyMetadata) -> JavaVersion {
    JavaVersion {
        major_version: full_data.java_version.major_version as u8,
    }
}

fn extract_arguments(full_data: &LightyMetadata) -> Arguments {
    Arguments {
        game: full_data.arguments.game.clone(),
        // Ne retourne Some que si les JVM args ne sont pas vides
        // Sinon retourne None pour laisser le loader de base gérer
        jvm: if full_data.arguments.jvm.is_empty() {
            None
        } else {
            Some(full_data.arguments.jvm.clone())
        },
    }
}

fn extract_libraries(full_data: &LightyMetadata) -> Vec<Library> {
    full_data
        .libraries
        .iter()
        .map(|lib| Library {
            name: lib.name.clone(),
            url: lib.url.clone(),
            path: lib.path.clone(),
            sha1: lib.sha1.clone(),
            size: lib.size,
        })
        .collect()
}

fn extract_mods(full_data: &LightyMetadata) -> Vec<Mods> {
    full_data
        .mods
        .iter()
        .map(|mod_| Mods {
            name: mod_.name.clone(),
            url: Some(mod_.url.clone()),
            path: Some(mod_.path.clone()),
            sha1: Some(mod_.sha1.clone()),
            size: Some(mod_.size),
        })
        .collect()
}

fn extract_natives(full_data: &LightyMetadata) -> Vec<Native> {
    full_data
        .natives
        .iter()
        .map(|native| Native {
            name: native.name.clone(),
            url: Some(native.url.clone()),
            path: Some(native.path.clone()),
            sha1: Some(native.sha1.clone()),
            size: Some(native.size),
        })
        .collect()
}

fn extract_client(full_data: &LightyMetadata) -> Client {
    Client {
        name: full_data.client.name.clone(),
        url: Some(full_data.client.url.clone()),
        path: Some(full_data.client.path.clone()),
        sha1: Some(full_data.client.sha1.clone()),
        size: Some(full_data.client.size),
    }
}

fn extract_assets(full_data: &LightyMetadata) -> AssetsFile {
    let mut objects = HashMap::new();

    for asset in &full_data.assets {
        objects.insert(
            asset.hash.clone(),
            Asset {
                hash: asset.hash.clone(),
                size: asset.size,
                url: asset.url.clone(),
            },
        );
    }

    AssetsFile { objects }
}

/// Merge les libraries : combine simplement les deux listes
fn merge_libraries(mut loader_libs: Vec<Library>, lighty_libs: Vec<Library>) -> Vec<Library> {
    loader_libs.extend(lighty_libs);
    loader_libs
}

/// Merge les arguments game et JVM
fn merge_arguments(loader_args: Arguments, lighty_args: Arguments) -> Arguments {
    Arguments {
        game: {
            let mut args = loader_args.game;
            args.extend(lighty_args.game);
            args
        },
        jvm: match (loader_args.jvm, lighty_args.jvm) {
            (Some(mut loader_jvm), Some(lighty_jvm)) => {
                loader_jvm.extend(lighty_jvm);
                Some(loader_jvm)
            }
            (Some(loader_jvm), None) => Some(loader_jvm),
            (None, Some(lighty_jvm)) => Some(lighty_jvm),
            (None, None) => None,
        },
    }
}

/// Merge les natives : combine simplement les deux listes
fn merge_natives(mut loader_natives: Vec<Native>, lighty_natives: Vec<Native>) -> Vec<Native> {
    loader_natives.extend(lighty_natives);
    loader_natives
}

/// Merge les assets : combine les HashMap (écrase automatiquement si même hash)
fn merge_assets(mut loader_assets: AssetsFile, lighty_assets: AssetsFile) -> AssetsFile {
    loader_assets.objects.extend(lighty_assets.objects);
    loader_assets
}
