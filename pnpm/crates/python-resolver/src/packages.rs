use crate::{lockfile::LockedWheel, metadata::WheelMetadata};
use pep440_rs::Version;
use pep508_rs::PackageName;
use std::collections::BTreeMap;

/// One version of a distribution, as an index offers it: the wheel this
/// target would install, and where its metadata can be read without the
/// wheel.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub wheel: LockedWheel,
    /// The digests of the wheel's `METADATA` served beside it
    /// (PEP 658/714), when the index says it is there. `None` is an index
    /// that publishes no such file, so the metadata has to come out of the
    /// wheel itself. An empty map is a file with no digests published.
    pub core_metadata: Option<BTreeMap<String, String>>,
}

/// What a resolution knows so far: which versions each distribution
/// offers, and what the versions it has looked at require.
///
/// A resolution starts empty and grows: [`crate::step`] names the one
/// distribution or version it still needs, the caller fetches it and
/// records it here, and the next step sees it.
#[derive(Debug, Default)]
pub struct Packages {
    pub candidates: BTreeMap<PackageName, BTreeMap<Version, Candidate>>,
    pub metadata: BTreeMap<(PackageName, Version), WheelMetadata>,
}

impl Packages {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
