//! Recognizing a next-format document.

use crate::ComVer;

/// Which shape a lockfile document's sections have.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// The shape its `lockfileVersion` names.
    OfVersion,
    /// Peer-suffixed, whatever its `lockfileVersion`: the current lockfile,
    /// which records the wanted lockfile's version but is always written
    /// suffixed, so that reading it needs no conversion.
    Suffixed,
}

/// Whether the document's `lockfileVersion` is the next format's. pnpm writes
/// the field on the document's first line, so this reads no further than the
/// first line that sets it.
pub(crate) fn declares_next_format(document: &str) -> bool {
    document
        .lines()
        .find_map(|line| line.strip_prefix("lockfileVersion:"))
        .map(|value| value.trim().trim_matches(['\'', '"']))
        .and_then(|value| value.parse::<ComVer>().ok())
        .is_some_and(|version| version.major == crate::NEXT_LOCKFILE_MAJOR)
}
