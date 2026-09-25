//! Failure modes for [`crate::pick_package()`].

use std::{fmt, path::PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;

use crate::{FetchMetadataError, pick_package_from_meta::PickPackageFromMetaError};

/// Failure modes for [`crate::pick_package()`]. Distinguishes the pure-pick
/// errors ([`PickPackageError::Pick`]) from the fetch / IO errors so
/// the install layer can route them through different reporters
/// (a missing time gets a warning; a network failure gets a retry
/// prompt).
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum PickPackageError {
    /// `ERR_PNPM_INVALID_PACKAGE_NAME`: a package name contains a `/`
    /// but doesn't begin with a `@scope/` prefix.
    #[display("Package name {pkg_name} is invalid, it should have a @scope")]
    InvalidPackageName {
        #[error(not(source))]
        pkg_name: String,
    },
    /// `ERR_PNPM_NO_OFFLINE_META`: offline mode is active and the
    /// on-disk mirror doesn't have the package.
    #[display("Failed to resolve {spec_name}@{spec_fetch_spec} in package mirror {pkg_mirror:?}")]
    NoOfflineMeta {
        #[error(not(source))]
        spec_name: String,
        spec_fetch_spec: String,
        pkg_mirror: PathBuf,
        /// Set when the pre-`#14081` mirror for the same registry still
        /// exists on disk, so the message can point at it. See
        /// `legacy_mirror_hint`.
        #[error(not(source))]
        hint: Option<String>,
    },
    /// Underlying picker error (no versions, unpublished, missing
    /// time, etc.). The picker errors are described on
    /// [`PickPackageFromMetaError`].
    Pick(PickPackageFromMetaError),
    /// Underlying metadata-fetch error (network, decode, 304 with
    /// no cache, etc.). Bubbles up from
    /// [`crate::fetch_full_metadata_cached()`].
    Fetch(FetchMetadataError),
}

/// Hand-rolled because [`PickPackageError::NoOfflineMeta`]'s help is
/// conditional on its `hint` field, which the derive macro cannot express.
/// `Pick` and `Fetch` forward every method to their inner error, replicating
/// what `#[diagnostic(transparent)]` generated before this hand roll.
impl Diagnostic for PickPackageError {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        match self {
            PickPackageError::InvalidPackageName { .. } => {
                Some(Box::new("ERR_PNPM_INVALID_PACKAGE_NAME"))
            }
            PickPackageError::NoOfflineMeta { .. } => Some(Box::new("ERR_PNPM_NO_OFFLINE_META")),
            PickPackageError::Pick(inner) => inner.code(),
            PickPackageError::Fetch(inner) => inner.code(),
        }
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        match self {
            PickPackageError::NoOfflineMeta { hint, .. } => hint
                .as_ref()
                .map(|hint| Box::new(hint) as Box<dyn fmt::Display + '_>),
            PickPackageError::Pick(inner) => inner.help(),
            PickPackageError::Fetch(inner) => inner.help(),
            _ => None,
        }
    }

    fn severity(&self) -> Option<miette::Severity> {
        match self {
            PickPackageError::Pick(inner) => inner.severity(),
            PickPackageError::Fetch(inner) => inner.severity(),
            _ => None,
        }
    }

    fn url(&self) -> Option<Box<dyn fmt::Display + '_>> {
        match self {
            PickPackageError::Pick(inner) => inner.url(),
            PickPackageError::Fetch(inner) => inner.url(),
            _ => None,
        }
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        match self {
            PickPackageError::Pick(inner) => inner.source_code(),
            PickPackageError::Fetch(inner) => inner.source_code(),
            _ => None,
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        match self {
            PickPackageError::Pick(inner) => inner.labels(),
            PickPackageError::Fetch(inner) => inner.labels(),
            _ => None,
        }
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        match self {
            PickPackageError::Pick(inner) => inner.related(),
            PickPackageError::Fetch(inner) => inner.related(),
            _ => None,
        }
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        match self {
            PickPackageError::Pick(inner) => inner.diagnostic_source(),
            PickPackageError::Fetch(inner) => inner.diagnostic_source(),
            _ => None,
        }
    }
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
