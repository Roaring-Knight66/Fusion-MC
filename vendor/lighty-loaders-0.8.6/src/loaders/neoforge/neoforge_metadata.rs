use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct NeoForgeMetaData {
    pub spec: i32,
    pub profile: String,
    pub version: String,
    pub icon: String,
    pub minecraft: String,
    pub json: String,
    pub logo: String,
    pub welcome: String,
    #[serde(rename = "mirrorList")]
    pub mirror_list: String,
    #[serde(rename = "hideExtract", default)]
    pub hide_extract: bool,
    pub data: HashMap<String, DataEntry>,
    pub processors: Vec<Processor>,
    pub libraries: Vec<NeoForgeLibrary>,
    #[serde(rename = "serverJarPath")]
    pub server_jar_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DataEntry {
    pub client: String,
    pub server: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Processor {
    #[serde(default)]
    pub sides: Vec<String>,
    pub jar: String,
    pub classpath: Vec<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NeoForgeLibrary {
    pub name: String,
    pub downloads: LibraryDownloads,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LibraryDownloads {
    pub artifact: Artifact,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Artifact {
    pub sha1: String,
    pub size: u64,
    pub url: String,
    pub path: String,
}

// ============= VERSION.JSON =============

#[derive(Debug, Deserialize, Clone)]
pub struct NeoForgeVersionMeta {
    pub id: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    #[serde(rename = "type")]
    pub release_type: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: String,
    pub arguments: NeoForgeArguments,
    pub libraries: Vec<NeoForgeLibrary>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NeoForgeArguments {
    pub game: Vec<String>,
    pub jvm: Vec<String>,
}
