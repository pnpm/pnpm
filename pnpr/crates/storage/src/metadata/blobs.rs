//! The types of the hosted store's archive bookkeeping.
//!
//! A namespace stores the bytes of an archive once, by their identity (see
//! [`crate::BlobId`]), and the metadata maps each public file name of a
//! package, such as `pkg-1.0.0.tgz`, to the identity holding its bytes. The
//! mapping is written by the commit that publishes the file and dropped by
//! the commit that removes its version or package.
//!
//! Each identity has one row in the `blobs` table: the incarnation its bytes
//! are stored under now, and that incarnation's state. A commit depends on
//! the incarnation it uploaded or found live, and is refused unless that
//! incarnation is live, or is the next one and the commit creates it. A
//! reclaim moves a live incarnation that nothing maps to `reclaiming` in one
//! conditional transaction, removes exactly that incarnation's key, and then
//! records it `gone`. The row is kept: incarnations only grow, so a key is
//! never written again once a reclaim may remove it, and a removal that lands
//! late, from any process, removes bytes nothing can reach.

use crate::BlobId;
use pnpr_package_name::Ecosystem;

/// Where an incarnation of a blob stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobState {
    /// Stored, and what reads of the files mapped to it serve.
    Live,
    /// Being removed: no commit depends on it, and its key is removed next.
    Reclaiming,
    /// Removed, or never committed and fenced off before its removal.
    Gone,
}

impl BlobState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Reclaiming => "reclaiming",
            Self::Gone => "gone",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Live, Self::Reclaiming, Self::Gone]
            .into_iter()
            .find(|state| state.as_str() == text)
    }
}

/// The row of a blob: its current incarnation and that incarnation's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobRow {
    pub incarnation: u64,
    pub state: BlobState,
    pub size: u64,
}

impl BlobRow {
    /// The incarnation a publish of these bytes stores them under. `row` is
    /// `None` when the bytes were never stored.
    #[must_use]
    pub fn incarnation_for_publish(row: Option<&Self>) -> PublishIncarnation {
        match row {
            Some(row) if row.state == BlobState::Live => PublishIncarnation::Reuse(row.incarnation),
            Some(row) => PublishIncarnation::Store(row.incarnation + 1),
            None => PublishIncarnation::Store(1),
        }
    }
}

/// The incarnation a publish stores its bytes under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishIncarnation {
    /// The live one, which it reuses without storing anything.
    Reuse(u64),
    /// The next one, which it stores.
    Store(u64),
}

/// An incarnation stored that no commit made live, as a settle takes it
/// (see [`crate::metadata::SqlMetadata::settle_stored`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncommitted {
    pub blob: BlobRef,
    /// Whether the settle may fence the incarnation off when a publish on
    /// its way could still commit it.
    pub may_fence: bool,
}

/// An incarnation of a blob.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobRef {
    pub id: BlobId,
    pub incarnation: u64,
}

impl BlobRef {
    /// The key this incarnation is stored under.
    #[must_use]
    pub fn key(&self) -> String {
        self.id.key(self.incarnation)
    }
}

/// A public file name of a package and the incarnation holding its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBlob {
    pub ecosystem: Ecosystem,
    /// The canonical package name.
    pub name: String,
    /// The version the file belongs to, empty when the metadata ties it to
    /// none, such as an image blob. Removing the version drops the mapping.
    pub version: String,
    /// The public file name.
    pub artifact: String,
    pub blob: BlobRef,
    pub size: u64,
}

/// What a commit maps and depends on in the hosted store, checked as it
/// commits, each file with the incarnation holding its bytes.
#[derive(Debug, Default, Clone)]
pub struct BlobFence {
    /// The files the commit stored, which it maps.
    pub artifacts: Vec<ArtifactBlob>,
    /// Files of a package mapped already that the commit's documents
    /// reference without storing them, such as the layers an image manifest
    /// names: each must still be mapped to its blob as the commit commits.
    pub references: Vec<ArtifactBlob>,
}

/// What a publish reads with the package's document: the rows of the blobs
/// it is about to store, and the mappings of files of the package it
/// references without storing them.
#[derive(Debug, Default, Clone)]
pub struct BlobLookup {
    pub ids: Vec<BlobId>,
    pub artifacts: Vec<String>,
}

/// A file of a package mapped to the blob holding its bytes, as a publish
/// read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedArtifact {
    pub artifact: String,
    pub version: String,
    pub id: BlobId,
    pub row: BlobRow,
}

/// A package's document read for a publish, with what its [`BlobLookup`]
/// found, read in the same round trip.
#[derive(Debug)]
pub struct PublishBase {
    pub stored: Option<crate::HostedDocumentForUpdate>,
    /// The row of each looked-up identity, `None` for bytes never stored.
    pub blobs: std::collections::BTreeMap<BlobId, Option<BlobRow>>,
    /// The mapping of each looked-up file the package has.
    pub artifacts: Vec<MappedArtifact>,
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
