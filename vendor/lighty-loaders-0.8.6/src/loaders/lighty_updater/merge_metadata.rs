use crate::loaders::fabric::fabric::FABRIC;
use crate::loaders::neoforge::neoforge::NEOFORGE;
use crate::loaders::quilt::quilt::QUILT;
use crate::loaders::vanilla::vanilla::VANILLA;
use crate::types::version_metadata::Version;
use crate::types::{Loader, VersionInfo};
use crate::utils::error::QueryError;

pub type Result<T> = std::result::Result<T, QueryError>;

/// Merge les métadonnées de Lighty Updater avec un autre loader
/// selon le loader spécifié dans ServerInfo
pub async fn merge_metadata<V: VersionInfo>(version: &V, loader: &str) -> Result<Version> {
    use super::lighty_updater::extract_version_builder;

    lighty_core::trace_debug!("🔀 [merge_metadata] START with loader={}", loader);

    let loader_metadata: Loader = match loader {
        "vanilla" => Loader::Vanilla,
        "fabric" => Loader::Fabric,
        "quilt" => Loader::Quilt,
        "neoforge" => Loader::NeoForge,
        _ => {
            lighty_core::trace_error!("❌ [merge_metadata] Unknown loader: {}", loader);
            return Err(QueryError::UnsupportedLoader(format!(
                "Unknown loader '{}' - please check your LightyUpdater config",
                loader
            )));
        }
    };

    lighty_core::trace_debug!("✅ [merge_metadata] Loader mapped: {:?}", loader_metadata);

    // Fetch le VersionBuilder du loader correspondant
    lighty_core::trace_debug!("📦 [merge_metadata] Fetching base loader data...");
    let merged_metadata = match loader_metadata {
        Loader::Vanilla => {
            lighty_core::trace_debug!("   -> Using Vanilla loader");
            use crate::loaders::vanilla::vanilla::VanillaQuery;
            let data = VANILLA.get(version, VanillaQuery::VanillaBuilder).await?;
            extract_version_builder(data)?
        }
        Loader::Fabric => {
            lighty_core::trace_debug!("   -> Using Fabric loader");
            use crate::loaders::fabric::fabric::FabricQuery;
            let data = FABRIC.get(version, FabricQuery::FabricBuilder).await?;
            extract_version_builder(data)?
        }
        Loader::Quilt => {
            lighty_core::trace_debug!("   -> Using Quilt loader");
            use crate::loaders::quilt::quilt::QuiltQuery;
            let data = QUILT.get(version, QuiltQuery::QuiltBuilder).await?;
            extract_version_builder(data)?
        }
        Loader::NeoForge => {
            lighty_core::trace_debug!("   -> Using NeoForge loader");
            use crate::loaders::neoforge::neoforge::NeoForgeQuery;
            let data = NEOFORGE
                .get(version, NeoForgeQuery::NeoForgeBuilder)
                .await?;
            extract_version_builder(data)?
        }
        _ => {
            lighty_core::trace_error!(
                "❌ [merge_metadata] Loader not supported: {:?}",
                loader_metadata
            );
            return Err(QueryError::UnsupportedLoader(format!(
                "Loader {:?} not supported for merging",
                loader_metadata
            )));
        }
    };

    lighty_core::trace_debug!("✅ [merge_metadata] Base loader data fetched successfully");
    Ok(merged_metadata)
}
