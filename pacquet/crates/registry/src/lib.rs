mod package;
mod package_distribution;
mod package_tag;
mod package_version;

pub use package::Package;
pub use package_distribution::PackageDistribution;
pub use package_tag::PackageTag;
pub use package_version::PackageVersion;

use derive_more::{Display, Error, From};
use miette::Diagnostic;

#[derive(Debug, Display, Error)]
#[display("Failed to request {url}: {error}")]
pub struct NetworkError {
    pub url: String,
    #[error(source)]
    pub error: reqwest::Error,
}

#[derive(Debug, Display, Error, From, Diagnostic)]
#[non_exhaustive]
pub enum RegistryError {
    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Missing latest tag on {_0}")]
    #[diagnostic(code(pacquet_registry::missing_latest_tag))]
    MissingLatestTag(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Missing version {_0} on package {_1}")]
    #[diagnostic(code(pacquet_registry::missing_version_release))]
    MissingVersionRelease(String, String),

    #[diagnostic(code(pacquet_registry::network_error))]
    Network(NetworkError), // TODO: remove derive(Error), split this variant

    #[diagnostic(code(pacquet_registry::io_error))]
    Io(std::io::Error), // TODO: remove derive(Error), split this variant

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Serialization failed: {_0}")]
    #[diagnostic(code(pacquet_registry::serialization_error))]
    Serialization(#[error(not(source))] String),
}
