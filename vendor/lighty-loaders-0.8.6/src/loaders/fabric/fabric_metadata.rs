use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct FabricMetaData {
    pub arguments: FabricArguments,
    pub id: String,
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: String,
    pub libraries: Vec<FabricLibrary>,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    pub time: String,
    #[serde(rename = "type")]
    pub version_type: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FabricArguments {
    pub game: Vec<String>,
    pub jvm: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FabricLibrary {
    pub name: String,
    pub url: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub sha512: Option<String>,
    pub size: Option<u64>,
}
