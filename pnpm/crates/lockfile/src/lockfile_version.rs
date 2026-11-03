use crate::ComVer;
use derive_more::{AsRef, Deref, Display, Error, Into};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The `lockfileVersion` major of a lockfile that records a registry
/// package's platform variants. pnpm releases that predate it refuse the
/// lockfile in every mode, so none of them rewrites it without its variants.
pub const PLATFORM_VARIANTS_LOCKFILE_MAJOR: u16 = 13;

/// The lockfile formats this pnpm reads, as it announces them to pnpr in a
/// resolve request's `acceptedLockfileFormats`.
pub const ACCEPTED_LOCKFILE_FORMATS: [&str; 3] = ["9.0", "12.0", "13.0"];

/// Wrapper that checks compatibility with the lockfile versions this client supports.
#[derive(
    Debug, Display, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsRef, Deref, Into,
)]
#[serde(try_from = "ComVer", into = "ComVer")]
pub struct LockfileVersion<const MAJOR: u16>(ComVer);

impl<const MAJOR: u16> LockfileVersion<MAJOR> {
    /// Check if `comver` is compatible with `MAJOR`.
    #[must_use]
    pub const fn is_compatible(comver: ComVer) -> bool {
        comver.major == MAJOR
            || MAJOR == 9
                && (comver.major == 12 || comver.major == PLATFORM_VARIANTS_LOCKFILE_MAJOR)
    }
}

/// Error when [`ComVer`] fails compatibility check.
#[derive(Debug, Error)]
pub enum LockfileVersionError<const MAJOR: u16> {
    IncompatibleMajor(#[error(not(source))] ComVer),
}

impl<const MAJOR: u16> fmt::Display for LockfileVersionError<MAJOR> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompatibleMajor(comver) => write!(
                f,
                "The lockfileVersion of {comver} is incompatible with this version of pnpm, \
                 which supports lockfileVersion {MAJOR}.x",
            ),
        }
    }
}

impl<const MAJOR: u16> TryFrom<ComVer> for LockfileVersion<MAJOR> {
    type Error = LockfileVersionError<MAJOR>;
    fn try_from(comver: ComVer) -> Result<Self, Self::Error> {
        Self::is_compatible(comver)
            .then_some(Self(comver))
            .ok_or(Self::Error::IncompatibleMajor(comver))
    }
}

#[cfg(test)]
mod tests;
