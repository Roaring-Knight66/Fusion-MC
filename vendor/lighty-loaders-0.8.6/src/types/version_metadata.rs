use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Pivot universel, utilisé par tous les loaders (Vanilla, Fabric, Forge...)
#[derive(Clone, Debug)]
pub enum VersionMetaData {
    JavaVersion(JavaVersion),
    MainClass(MainClass),
    Libraries(Vec<Library>),
    Mods(Vec<Mods>),
    Natives(Vec<Native>),
    AssetsIndex(AssetIndex),
    Assets(AssetsFile),
    Client(Client),
    Arguments(Arguments),
    Version(Version),
}

/// Structure complète contenant toutes les métadonnées d'une version
#[derive(Debug, Clone)]
pub struct Version {
    pub main_class: MainClass,
    pub java_version: JavaVersion,
    pub arguments: Arguments,
    pub libraries: Vec<Library>,
    pub mods: Option<Vec<Mods>>,
    pub natives: Option<Vec<Native>>,
    pub client: Option<Client>,
    pub assets_index: Option<AssetIndex>,
    pub assets: Option<AssetsFile>,
}

#[derive(Debug, Clone)]
pub struct MainClass {
    pub main_class: String,
}

#[derive(Debug, Clone)]
pub struct JavaVersion {
    pub major_version: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arguments {
    pub game: Vec<String>,
    pub jvm: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mods {
    pub name: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Native {
    pub name: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub name: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetsFile {
    pub objects: HashMap<String, Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub hash: String,
    pub size: u64,
    pub url: Option<String>,
}
