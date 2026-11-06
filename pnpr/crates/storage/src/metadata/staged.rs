//! The records of staged archives: a principal's archive uploaded by digest
//! ahead of the commit that claims it (see [`crate::StagedArchiveKey`]).
//!
//! The bytes stay in the hosted store, under `<digest>.<incarnation>`. The
//! record counts incarnations up like a blob's row (see
//! [`crate::metadata::BlobRow`]): once a sweep records an incarnation
//! `gone`, a later upload of the same bytes stores the next one, so the
//! sweep's removal of the old key can land at any time.

/// Where a staged archive's current incarnation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedState {
    /// Reserved for an upload whose bytes are not all stored yet, or given
    /// back.
    Incomplete,
    /// Stored, and claimable while it is young enough.
    Complete,
    /// Swept: its key is removed or about to be, and never written again.
    Gone,
}

impl StagedState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Incomplete => "incomplete",
            Self::Complete => "complete",
            Self::Gone => "gone",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Incomplete, Self::Complete, Self::Gone]
            .into_iter()
            .find(|state| state.as_str() == text)
    }
}

/// The record of one principal's staged archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StagedRecord {
    /// The incarnation its bytes are stored under.
    pub(crate) incarnation: u64,
    /// When it was last uploaded, in seconds since the Unix epoch; `0` once
    /// given back or claimed.
    pub(crate) staged_at: u64,
    pub(crate) size: u64,
    pub(crate) state: StagedState,
}
