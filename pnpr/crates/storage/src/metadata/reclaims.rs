//! The types of archive reclamation: removing an archive no package
//! references while replicas keep publishing.
//!
//! The metadata database fences the removal. A reclaim first marks the
//! archives it will remove, in a transaction that holds only while their
//! package is still the one the reclaim found them unreferenced in. It then
//! removes them from the hosted store, and records the catalog generation
//! that saw them go. A commit names the archives it depends on and the
//! catalog generation it read before it promoted them, and is refused while
//! one of them is marked, or when one went after that generation: the bytes
//! it promoted may be the ones removed.

use pnpr_package_name::Ecosystem;

/// One archive of a package in a catalog.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveRef {
    pub ecosystem: Ecosystem,
    /// The canonical package name.
    pub name: String,
    /// The archive's file name in the package's blob store.
    pub artifact: String,
}

/// What a commit depends on in the hosted store, checked as it commits.
#[derive(Debug, Default, Clone)]
pub struct ArchiveFence {
    /// The catalog generation the publish read before it promoted
    /// `archives`. A reclaim done after it may have removed what was
    /// promoted.
    pub token: u64,
    /// Every archive the commit's documents reference that the publish
    /// promoted or found in place.
    pub archives: Vec<ArchiveRef>,
}

/// Archives of one package to reclaim, while the package is still in the
/// state they were found unreferenced in.
#[derive(Debug, Clone)]
pub struct ReclaimTarget {
    pub ecosystem: Ecosystem,
    /// The canonical package name.
    pub name: String,
    /// The package's generation when its archives were found unreferenced,
    /// or `None` when it had no metadata.
    pub expected: Option<u64>,
    pub artifacts: Vec<String>,
}

/// A package's document read for a publish, with the catalog generation
/// the publish fences its archives with.
#[derive(Debug)]
pub struct PublishBase {
    pub stored: Option<crate::HostedDocumentForUpdate>,
    /// The catalog's generation, read with the document.
    pub catalog_generation: u64,
}

impl PublishBase {
    /// The package's generation, or `None` when it has no metadata.
    #[must_use]
    pub fn package_generation(&self) -> Option<u64> {
        match self.stored.as_ref()?.version {
            crate::HostedDocumentVersion::Generation(generation) => Some(generation),
            _ => None,
        }
    }
}
