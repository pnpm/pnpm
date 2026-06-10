//! Error types for the npm verifier's network / parsing surface.

use derive_more::{Display, Error};
use miette::Diagnostic;

/// Failure to fetch a registry metadata document. Used by
/// [`crate::fetch_full_metadata()`] and
/// [`crate::fetch_full_metadata_cached()`]; flows up through the
/// verifier's `verify` and is folded into a violation with
/// [`crate::MINIMUM_RELEASE_AGE_VIOLATION_CODE`] or
/// [`crate::TRUST_DOWNGRADE_VIOLATION_CODE`] depending on which
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

    #[display("Failed to decode metadata from {url}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::decode_error))]
    Decode {
        url: String,
        #[error(source)]
        error: serde_json::Error,
    },

    /// Mirrors upstream's `META_NOT_MODIFIED_WITHOUT_CACHE`. Surfaces
    /// only when a stale-but-removed mirror plus an `If-None-Match`
    /// header the caller-provided cache headers carried (impossible
    /// in pacquet's chain because we always read headers off a
    /// present mirror) would trip a 304 reply we have no body to
    /// satisfy — a defense-in-depth check for hand-edited caches.
    #[display("Registry returned 304 for {pkg_name} without an existing cache to refresh.")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::not_modified_without_cache))]
    NotModifiedWithoutCache {
        #[error(not(source))]
        pkg_name: String,
    },

    /// Mirrors upstream's `META_CACHE_MISSING_AFTER_304`. The mirror
    /// existed when we read its headers but vanished before the
    /// full read on a 304 response — concurrent cache cleanup,
    /// antivirus, etc.
    #[display("Metadata cache for {pkg_name} disappeared between headers read and full read.")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::cache_missing_after_304))]
    CacheMissingAfter304 {
        #[error(not(source))]
        pkg_name: String,
    },

    /// The blocking task that deserializes a packument body panicked
    /// or was cancelled by runtime shutdown.
    #[display("Failed to parse metadata from {url}: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::parse_task))]
    ParseTask {
        url: String,
        #[error(source)]
        error: tokio::task::JoinError,
    },
}
