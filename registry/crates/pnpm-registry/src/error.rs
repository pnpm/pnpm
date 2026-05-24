use derive_more::{Display, Error, From};

#[derive(Debug, Display, Error, From)]
#[non_exhaustive]
pub enum RegistryError {
    #[display("Upstream request to {url} failed: {source}")]
    Upstream {
        url: String,
        #[error(source)]
        source: reqwest::Error,
    },

    #[display("Upstream returned status {status} for {url}")]
    UpstreamStatus {
        url: String,
        status: u16,
        #[error(not(source))]
        body: String,
    },

    #[display("Package name {name:?} is not a valid npm package name")]
    InvalidPackageName {
        #[error(not(source))]
        name: String,
    },

    #[display("Tarball filename {filename:?} is not valid for package {package:?}")]
    InvalidTarballName {
        #[error(not(source))]
        package: String,
        filename: String,
    },

    #[display("I/O error: {_0}")]
    Io(std::io::Error),

    #[display("JSON error: {_0}")]
    Json(serde_json::Error),
}

pub type Result<T, E = RegistryError> = std::result::Result<T, E>;
