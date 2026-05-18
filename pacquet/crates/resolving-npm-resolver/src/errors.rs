//! Error types for the npm verifier's network / parsing surface.

use derive_more::{Display, Error};
use miette::Diagnostic;

/// Failure to fetch a registry metadata document. Used by
/// [`crate::fetch_full_metadata`] and (in Phase 5) the cached
/// fetcher; flows up through the verifier's `verify` and is folded
/// into a violation with [`crate::MINIMUM_RELEASE_AGE_VIOLATION_CODE`]
/// or [`crate::TRUST_DOWNGRADE_VIOLATION_CODE`] depending on which
/// policy triggered the lookup.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FetchMetadataError {
    #[display("Failed to fetch metadata from {url}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::network_error))]
    Network {
        url: String,
        #[error(source)]
        error: reqwest::Error,
    },
}
