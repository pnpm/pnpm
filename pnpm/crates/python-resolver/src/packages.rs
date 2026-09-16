use crate::{
    lockfile::{LockedDirectory, LockedWheel},
    metadata::WheelMetadata,
};
use pep440_rs::Version;
use pep508_rs::PackageName;
use std::collections::BTreeMap;

/// Where one version of a distribution comes from: a wheel an index
/// serves, or a directory in this workspace.
#[derive(Debug, Clone)]
pub enum Candidate {
    Wheel(IndexCandidate),
    /// A project in this workspace, which is built from its source rather
    /// than downloaded. Its metadata is read from its own manifest.
    Directory(LockedDirectory),
}

/// The wheel an index serves for one version, and where its metadata can
/// be read without downloading it.
#[derive(Debug, Clone)]
pub struct IndexCandidate {
    pub wheel: LockedWheel,
    /// The digests of the wheel's `METADATA` served beside it
    /// (PEP 658/714), when the index says it is there. `None` is an index
    /// that publishes no such file, so the metadata has to come out of the
    /// wheel itself. An empty map is a file with no digests published.
    pub core_metadata: Option<BTreeMap<String, String>>,
}

impl Candidate {
    /// What an index serves for this candidate, or `None` for a
    /// directory.
    #[must_use]
    pub fn from_index(&self) -> Option<&IndexCandidate> {
        match self {
            Self::Wheel(candidate) => Some(candidate),
            Self::Directory(_) => None,
        }
    }

    /// The wheel this candidate installs, or `None` for a directory.
    #[must_use]
    pub fn wheel(&self) -> Option<&LockedWheel> {
        match self {
            Self::Wheel(candidate) => Some(&candidate.wheel),
            Self::Directory(_) => None,
        }
    }

    /// The workspace project this candidate builds, or `None` for a wheel.
    #[must_use]
    pub fn directory(&self) -> Option<&LockedDirectory> {
        match self {
            Self::Wheel(_) => None,
            Self::Directory(directory) => Some(directory),
        }
    }

    /// The digests of the `METADATA` an index serves beside this
    /// candidate's wheel, when it does. A directory has none: its
    /// metadata is read from its manifest.
    #[must_use]
    pub fn core_metadata(&self) -> Option<&BTreeMap<String, String>> {
        match self {
            Self::Wheel(candidate) => candidate.core_metadata.as_ref(),
            Self::Directory(_) => None,
        }
    }

    /// The name a diagnostic calls this candidate by.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Wheel(candidate) => &candidate.wheel.name,
            Self::Directory(directory) => &directory.path,
        }
    }
}

/// What a resolution knows so far: which versions each distribution
/// offers, and what the versions it has looked at require.
///
/// A resolution starts empty and grows: [`crate::step`] names the one
/// distribution or version it still needs, the caller fetches it and
/// records it here, and the next step sees it.
///
/// A workspace project is seeded before the resolution starts: its
/// version and requirements are in its manifest, so there is nothing to
/// fetch and nothing for a step to ask for.
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

    /// Offer a workspace project as the only version of its distribution,
    /// with the metadata its manifest declares.
    pub fn insert_directory(
        &mut self,
        name: PackageName,
        version: Version,
        directory: LockedDirectory,
        metadata: WheelMetadata,
    ) {
        self.candidates.insert(
            name.clone(),
            BTreeMap::from([(version.clone(), Candidate::Directory(directory))]),
        );
        self.metadata.insert((name, version), metadata);
    }
}
