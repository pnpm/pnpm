//! The hosted packages' metadata, split from their archives.
//!
//! [`SqlMetadata`] keeps accepted versions, tags, deprecations, and package
//! state as rows of a SQL database and renders documents from them. The
//! registry's handlers reach one namespace of it through [`Storage`]: reading
//! a package view, committing a package document, changing a tag, removing a
//! package. Archives and the records beside them stay in the hosted backend;
//! the metadata owns only what a package's document says about them.

pub(crate) mod sql;

pub use blobs::{
    ArtifactBlob, BlobFence, BlobLookup, BlobRef, BlobRow, BlobState, MappedArtifact, PublishBase,
    PublishIncarnation, Uncommitted,
};
pub use notes::{
    MAX_NOTE_BYTES, Note, NoteChange, PublishNotes, VersionNote, note_digest, validate_note,
};
pub use publication::{
    ArtifactMember, BatchDocument, Publication, PublicationCommit, PublicationRequest,
    RecordedOutcome, ReleaseRecords,
};
pub(crate) use sql::PackageMetadata;
pub use sql::{Mapping, RenderStats, SqlMetadata, TagHistoryEntry, Unmapping};
pub(crate) use sql::{StagedEntry, StagedId};
pub(crate) use staged::{StagedRecord, StagedState};

mod blobs;
mod notes;
mod publication;
mod staged;

use crate::{DocumentUpdate, DocumentWrite, Storage, wait_after_document_write_conflict};
use pnpr_config::MetadataBackend;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use std::{fmt, sync::Arc};

/// The field of a stored npm version that records the interpretation profile
/// its manifest was accepted under, as `profile`, and the digests of the raw
/// inputs the profile derived it from.
pub const ARCHIVE_MANIFEST_FIELD: &str = "_archiveManifest";

/// A committed state of one hosted catalog: the organization namespace a
/// package lives in. Every committed mutation advances its catalog's counter,
/// so two generations of one catalog order its commits. Counters of different
/// catalogs are unrelated and never compared.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Generation {
    /// The organization namespace; empty for the default one.
    pub catalog: String,
    pub value: u64,
}

impl fmt::Display for Generation {
    /// `<catalog>:<value>`, the token a response carries.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.catalog, self.value)
    }
}

/// A package's document as a reader receives it.
#[derive(Debug, Clone)]
pub struct PackageView {
    pub bytes: Arc<[u8]>,
    /// The generation the view was rendered from.
    pub generation: Generation,
}

/// A dist-tag change: point `tag` at `target`, or remove it when `target` is
/// `None`.
#[derive(Debug, Clone, Copy)]
pub struct TagChange<'change> {
    pub tag: &'change str,
    pub target: Option<&'change str>,
    /// Who asked for the change, recorded in the tag's history.
    pub principal: Option<&'change str>,
}

/// Outcome of [`Storage::change_dist_tag`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagUpdate {
    /// The package has no document to tag.
    NotFound,
    /// The tag points where the change asked. `generation` is the one that
    /// holds it; `moved` says whether this change moved it, rather than
    /// finding it there.
    Committed { generation: Option<Generation>, moved: bool },
}

/// Open the metadata database `backend` names.
pub(crate) fn open(backend: &MetadataBackend) -> Result<Arc<SqlMetadata>> {
    match backend {
        MetadataBackend::Sqlite(path) => Ok(Arc::new(SqlMetadata::open_sqlite(path)?)),
        MetadataBackend::Libsql(settings) => open_libsql(settings),
    }
}

/// Read a document, transform it, and write it back while the store still
/// holds what was read, retrying on conflict with capped backoff.
///
/// `build` receives the current bytes (`None` when the document is absent)
/// and returns the bytes to write, or `Ok(None)` to stop with
/// [`DocumentUpdate::NotFound`]; a `build` error stops without retrying. After
/// `retries` conflicts the write fails with
/// [`RegistryError::DocumentWriteConflict`].
pub(crate) async fn update_with_retry<Build>(
    metadata: &PackageMetadata,
    name: &CanonicalPackageName,
    retries: usize,
    principal: Option<&str>,
    mut build: Build,
) -> Result<DocumentUpdate>
where
    Build: FnMut(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
{
    for attempt in 0..retries {
        let (existing, version) = match metadata.read_for_update(name).await? {
            Some(document) => (Some(document.bytes), Some(document.version)),
            None => (None, None),
        };
        let Some(new_bytes) = build(existing.as_deref())? else {
            return Ok(DocumentUpdate::NotFound);
        };
        match metadata.write_if_current(name, &new_bytes, version.as_ref(), principal).await? {
            DocumentWrite::Written => return Ok(DocumentUpdate::Written),
            DocumentWrite::Conflict if attempt + 1 < retries => {
                wait_after_document_write_conflict(attempt).await;
            }
            DocumentWrite::Conflict => {}
        }
    }
    Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
}

/// The libSQL metadata `settings` name, shared by every [`Storage`] of this
/// process that names it. An embedded replica is a file one database handle
/// keeps in sync, and what this process committed is tracked per handle.
#[cfg(feature = "backend-libsql")]
fn open_libsql(settings: &pnpr_config::LibsqlSettings) -> Result<Arc<SqlMetadata>> {
    type Opened = Vec<(pnpr_config::LibsqlSettings, std::sync::Weak<SqlMetadata>)>;
    static OPENED: std::sync::Mutex<Opened> = std::sync::Mutex::new(Vec::new());
    let mut opened = OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    opened.retain(|(_, database)| database.strong_count() > 0);
    if let Some(database) = opened
        .iter()
        .find(|(open, _)| open == settings)
        .and_then(|(_, database)| database.upgrade())
    {
        return Ok(database);
    }
    let database = Arc::new(SqlMetadata::open_libsql(settings));
    opened.push((settings.clone(), Arc::downgrade(&database)));
    Ok(database)
}

#[cfg(not(feature = "backend-libsql"))]
fn open_libsql(_settings: &pnpr_config::LibsqlSettings) -> Result<Arc<SqlMetadata>> {
    Err(pnpr_error::RegistryError::InvalidConfig {
        reason: "this pnpr was built without libSQL, so it cannot use a libSQL metadata URL"
            .to_string(),
    })
}

impl Storage {
    /// Read the package's document with the generation it was rendered from.
    pub async fn read_hosted_package_view(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<PackageView>> {
        self.metadata.read_view(name).await
    }

    /// [`Self::read_hosted_package_view`] at catalog generation `generation`
    /// or later, such as the generation a client has already seen. With an
    /// embedded replica behind it, the read waits for the replica to sync or
    /// reads the primary.
    pub async fn read_hosted_package_view_at_least(
        &self,
        name: &CanonicalPackageName,
        generation: u64,
    ) -> Result<Option<PackageView>> {
        self.metadata.read_view_at_least(name, generation).await
    }

    /// One version's accepted manifest, without its README and deprecation.
    /// Only that version's rows are read, so a release record never costs
    /// the package view.
    pub async fn read_hosted_version(
        &self,
        name: &CanonicalPackageName,
        version: &str,
    ) -> Result<Option<Arc<str>>> {
        self.metadata.read_version(name, version).await
    }

    /// The exact bytes of the release record of `revision` of `version`, as
    /// the version was accepted with it.
    pub async fn read_release_record(
        &self,
        name: &CanonicalPackageName,
        version: &str,
        revision: u64,
    ) -> Result<Option<Vec<u8>>> {
        self.metadata.read_release_record(name, version, revision).await
    }

    /// Move or remove a dist-tag of a hosted package in one metadata commit.
    pub async fn change_dist_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate> {
        self.metadata.change_tag(name, change).await
    }

    /// The metadata database.
    #[must_use]
    pub fn sql_metadata(&self) -> &Arc<SqlMetadata> {
        &self.database
    }
}
