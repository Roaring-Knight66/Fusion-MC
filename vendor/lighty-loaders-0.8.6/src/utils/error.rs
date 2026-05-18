use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    JsonParsing(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Version '{version}' not found in manifest")]
    VersionNotFound { version: String },

    #[error("Missing field '{field}' in manifest data")]
    MissingField { field: String },

    #[error("Assets fetch error: failed to fetch assets from URL '{url}'")]
    AssetsFetch { url: String },

    #[error("Conversion error: {message}")]
    Conversion { message: String },

    #[error("Unsupported loader: {0}")]
    UnsupportedLoader(String),

    #[error("Invalid metadata format")]
    InvalidMetadata,
}

pub type Result<T> = std::result::Result<T, QueryError>;
