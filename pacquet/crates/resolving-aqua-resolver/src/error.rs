//! Error type for the aqua resolver. Codes mirror the `PnpmError`
//! codes upstream raises in
//! [`resolving/aqua-resolver`](https://github.com/pnpm/pnpm/pull/10970).

use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;

#[derive(Debug, Display, Error, Diagnostic)]
pub enum AquaResolverError {
    #[display("Cannot resolve aqua packages in offline mode")]
    #[diagnostic(code(AQUA_OFFLINE))]
    Offline,

    #[display("Invalid aqua specifier \"{specifier}\". Expected format: aqua:owner/repo[@version]")]
    #[diagnostic(code(AQUA_INVALID_SPECIFIER))]
    InvalidSpecifier {
        #[error(not(source))]
        specifier: String,
    },

    #[display("Version \"{version_spec}\" not found for {owner}/{repo}")]
    #[diagnostic(code(AQUA_VERSION_NOT_FOUND))]
    VersionNotFound {
        #[error(not(source))]
        owner: String,
        #[error(not(source))]
        repo: String,
        #[error(not(source))]
        version_spec: String,
    },

    #[display("Failed to fetch {what}: {status}")]
    #[diagnostic(code(AQUA_GITHUB_FETCH))]
    GitHubFetch {
        #[error(not(source))]
        what: String,
        #[error(not(source))]
        status: u16,
    },

    #[display("Failed to fetch aqua registry for {owner}/{repo}: {status}")]
    #[diagnostic(code(AQUA_REGISTRY_FETCH))]
    RegistryFetch {
        #[error(not(source))]
        owner: String,
        #[error(not(source))]
        repo: String,
        #[error(not(source))]
        status: u16,
    },

    #[display("Failed to parse aqua registry for {owner}/{repo}")]
    #[diagnostic(code(AQUA_REGISTRY_PARSE))]
    RegistryParse {
        #[error(not(source))]
        owner: String,
        #[error(not(source))]
        repo: String,
        #[error(source)]
        error: Arc<serde_saphyr::Error>,
    },

    #[display("The aqua registry for {owner}/{repo} contains no packages")]
    #[diagnostic(code(AQUA_REGISTRY_PARSE))]
    RegistryEmpty {
        #[error(not(source))]
        owner: String,
        #[error(not(source))]
        repo: String,
    },

    #[display("No downloadable assets found for {owner}/{repo}@{version}")]
    #[diagnostic(code(AQUA_NO_ASSETS))]
    NoAssets {
        #[error(not(source))]
        owner: String,
        #[error(not(source))]
        repo: String,
        #[error(not(source))]
        version: String,
    },

    #[display("Failed to fetch {url}")]
    #[diagnostic(code(AQUA_FETCH))]
    Network {
        #[error(not(source))]
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Failed to parse integrity {integrity} for {asset_name}")]
    #[diagnostic(code(AQUA_PARSE_INTEGRITY))]
    Integrity {
        #[error(not(source))]
        asset_name: String,
        #[error(not(source))]
        integrity: String,
        #[error(source)]
        error: Arc<ssri::Error>,
    },
}
