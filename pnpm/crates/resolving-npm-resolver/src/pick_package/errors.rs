//! Failure modes for [`crate::pick_package()`].

use std::path::PathBuf;

use derive_more::{Display, Error};
use miette::Diagnostic;

use crate::{FetchMetadataError, pick_package_from_meta::PickPackageFromMetaError};

/// Failure modes for [`crate::pick_package()`]. Distinguishes the pure-pick
/// errors ([`PickPackageError::Pick`]) from the fetch / IO errors so
/// the install layer can route them through different reporters
/// (a missing time gets a warning; a network failure gets a retry
/// prompt).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PickPackageError {
    /// `ERR_PNPM_INVALID_PACKAGE_NAME`: a package name contains a `/`
    /// but doesn't begin with a `@scope/` prefix.
    #[display("Package name {pkg_name} is invalid, it should have a @scope")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        pkg_name: String,
    },
    /// `ERR_PNPM_NO_OFFLINE_META`: offline mode is active and the
    /// on-disk mirror doesn't have the package.
    #[display("Failed to resolve {spec_name}@{spec_fetch_spec} in package mirror {pkg_mirror:?}")]
    #[diagnostic(code(ERR_PNPM_NO_OFFLINE_META))]
    NoOfflineMeta {
        #[error(not(source))]
        spec_name: String,
        spec_fetch_spec: String,
        pkg_mirror: PathBuf,
        /// Set when the pre-`#14081` mirror for the same registry still
        /// exists on disk, so the message can point at it. See
        /// `legacy_mirror_hint`.
        #[error(not(source))]
        #[help]
        hint: Option<String>,
    },
    /// Underlying picker error (no versions, unpublished, missing
    /// time, etc.). The picker errors are described on
    /// [`PickPackageFromMetaError`].
    #[diagnostic(transparent)]
    Pick(PickPackageFromMetaError),
    /// Underlying metadata-fetch error (network, decode, 304 with
    /// no cache, etc.). Bubbles up from
    /// [`crate::fetch_full_metadata_cached()`].
    #[diagnostic(transparent)]
    Fetch(FetchMetadataError),
}

impl From<PickPackageFromMetaError> for PickPackageError {
    fn from(error: PickPackageFromMetaError) -> Self {
        PickPackageError::Pick(error)
    }
}

impl From<FetchMetadataError> for PickPackageError {
    fn from(error: FetchMetadataError) -> Self {
        PickPackageError::Fetch(error)
    }
}
