use crate::types::version_metadata::Version;
use crate::types::VersionInfo;
use crate::utils::error::QueryError;
use async_trait::async_trait;
use std::hash::Hash;
use std::time::Duration;

pub type Result<T> = std::result::Result<T, QueryError>;

#[async_trait]
pub trait Query: Send + Sync {
    /// Type des queries (VanillaQuery, ForgeQuery etc)
    type Query: Eq + Hash + Clone + Send + Sync + 'static;
    /// Type des données extraites (VanillaData, ForgeData etc) principalement VersionMetaData
    type Data: Clone + Send + Sync + 'static;

    /// Type brut renvoyé par fetch_full_data (JSON ou struct déjà typé)
    type Raw: Send + Sync + 'static;

    /// Nom du groupe de la query (vanilla, forge, custom…)
    fn name() -> &'static str;

    /// Récupère le manifest brut depuis une source externe
    async fn fetch_full_data<V: VersionInfo>(version: &V) -> Result<Self::Raw>;

    /// Transforme le manifest brut en donnée typée
    async fn extract<V: VersionInfo>(
        version: &V,
        query: &Self::Query,
        raw: &Self::Raw,
    ) -> Result<Self::Data>;

    /// TTL globale par défaut
    fn cache_ttl() -> Duration {
        Duration::from_secs(3600) // par défaut 1h
    }

    /// TTL spécifique par query (par défaut = cache_ttl)
    fn cache_ttl_for_query(_query: &Self::Query) -> Duration {
        Self::cache_ttl()
    }
    async fn version_builder<V: VersionInfo>(version: &V, full_data: &Self::Raw)
        -> Result<Version>;
}

/// Clé pour le cache
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryKey<Q> {
    pub version: String,
    pub query: Q,
}
