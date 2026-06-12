use crate::ComVer;
use derive_more::{AsRef, Deref, Display, Error, Into};
use serde::{Deserialize, Serialize};

/// Wrapper that checks compatibility of `lockfileVersion` against `MAJOR`.
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
    }
}

/// Error when [`ComVer`] fails compatibility check.
#[derive(Debug, Display, Error)]
pub enum LockfileVersionError<const MAJOR: u16> {
    #[display("The lockfileVersion of {_0} is incompatible with {MAJOR}.x")]
    IncompatibleMajor(#[error(not(source))] ComVer),
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
