use crate::loaders::fabric::fabric::{FabricQuery, FABRIC};
use crate::loaders::lighty_updater::lighty_updater::{LightyQuery, LIGHTY_UPDATER};
use crate::loaders::neoforge::neoforge::{NeoForgeQuery, NEOFORGE};
use crate::loaders::quilt::quilt::{QuiltQuery, QUILT};
use crate::loaders::vanilla::vanilla::{VanillaQuery, VANILLA};
use crate::types::version_metadata::VersionMetaData;
use crate::types::{Loader, VersionInfo};
use crate::utils::error::QueryError;
use async_trait::async_trait;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, QueryError>;

/// Trait pour étendre VersionBuilder avec les méthodes spécifiques aux loaders
#[async_trait]
pub trait LoaderExtensions {
    async fn get_library(&self) -> Result<Arc<VersionMetaData>>;

    // Vanilla
    async fn get_main_class(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_libraries(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_natives(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_java_version(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_assets(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_complete(&self) -> Result<Arc<VersionMetaData>>;

    // Fabric
    async fn get_fabric_libraries(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_fabric_complete(&self) -> Result<Arc<VersionMetaData>>;

    // Quilt
    async fn get_quilt_libraries(&self) -> Result<Arc<VersionMetaData>>;
    async fn get_quilt_complete(&self) -> Result<Arc<VersionMetaData>>;

    // NeoForge
    async fn get_neoforge_complete(&self) -> Result<Arc<VersionMetaData>>;

    // LightyUpdater
    async fn get_lighty_updater_complete(&self) -> Result<Arc<VersionMetaData>>;
}

// Generic implementation for any type that implements VersionInfo<LoaderType = Loader>
#[async_trait]
impl<T> LoaderExtensions for T
where
    T: VersionInfo<LoaderType = Loader> + Send + Sync,
{
    async fn get_library(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::Libraries).await
    }

    async fn get_main_class(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::MainClass).await
    }

    async fn get_libraries(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::Libraries).await
    }

    async fn get_natives(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::Natives).await
    }

    async fn get_java_version(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::JavaVersion).await
    }

    async fn get_assets(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::Assets).await
    }

    async fn get_complete(&self) -> Result<Arc<VersionMetaData>> {
        VANILLA.get(self, VanillaQuery::VanillaBuilder).await
    }

    async fn get_fabric_libraries(&self) -> Result<Arc<VersionMetaData>> {
        FABRIC.get(self, FabricQuery::Libraries).await
    }

    async fn get_fabric_complete(&self) -> Result<Arc<VersionMetaData>> {
        FABRIC.get(self, FabricQuery::FabricBuilder).await
    }

    async fn get_quilt_libraries(&self) -> Result<Arc<VersionMetaData>> {
        QUILT.get(self, QuiltQuery::Libraries).await
    }

    async fn get_quilt_complete(&self) -> Result<Arc<VersionMetaData>> {
        QUILT.get(self, QuiltQuery::QuiltBuilder).await
    }

    async fn get_neoforge_complete(&self) -> Result<Arc<VersionMetaData>> {
        NEOFORGE.get(self, NeoForgeQuery::NeoForgeBuilder).await
    }

    async fn get_lighty_updater_complete(&self) -> Result<Arc<VersionMetaData>> {
        LIGHTY_UPDATER.get(self, LightyQuery::LightyBuilder).await
    }
}
