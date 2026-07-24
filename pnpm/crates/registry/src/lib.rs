mod package;
mod package_distribution;
mod package_tag;
mod package_version;
mod package_versions;
mod pinned_version;

pub use package::Package;
pub use package_distribution::{AttestationsDist, PackageDistribution, ProvenanceMeta};
pub use package_tag::PackageTag;
pub use package_version::{Approver, NpmUser, PackageVersion, TrustedPublisher};
pub use package_versions::PackageVersions;
pub use pinned_version::PinnedVersion;

use derive_more::{Display, Error};
use miette::Diagnostic;

#[derive(Debug, Display, Error)]
#[display("Failed to request {url}: {error}")]
pub struct NetworkError {
    pub url: String,
    #[error(source)]
    pub error: reqwest::Error,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RegistryError {
    #[display("Missing latest tag on {_0}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_MISSING_LATEST_TAG))]
    MissingLatestTag(#[error(not(source))] String),

    #[display("Missing version {_0} on package {_1}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_MISSING_VERSION_RELEASE))]
    MissingVersionRelease(String, String),

    #[diagnostic(code(ERR_PNPM_REGISTRY_NETWORK_ERROR))]
    // TODO: remove derive(Error), split this variant into its own error type
    Network(NetworkError),

    #[diagnostic(code(ERR_PNPM_REGISTRY_IO_ERROR))]
    // TODO: remove derive(Error), split this variant into its own error type
    Io(std::io::Error),

    #[display("Serialization failed: {_0}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_SERIALIZATION_ERROR))]
    Serialization(#[error(not(source))] String),
}

impl From<NetworkError> for RegistryError {
    fn from(err: NetworkError) -> Self {
        Self::Network(err)
    }
}

impl From<std::io::Error> for RegistryError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
