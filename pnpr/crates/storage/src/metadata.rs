//! The hosted packages' metadata, split from their archives.
//!
//! `PackageMetadata` is the domain interface the registry's handlers reach
//! through [`Storage`]: reading a package view, committing a package document,
//! changing a tag, removing a package. Archives and the records beside them
//! stay in the hosted backend; a metadata backend owns only what a package's
//! document says about them.
//!
//! - The documents backend keeps one document per package in the hosted
//!   store, beside its archives.
//! - [`SqliteMetadata`] keeps accepted versions, tags, deprecations, and
//!   package state as rows of an embedded database and renders documents from
//!   them.

pub(crate) mod sqlite;

pub(crate) use documents::update_with_retry;
pub use publication::{
    ArtifactMember, BatchDocument, Publication, PublicationCommit, PublicationOrigin,
    PublicationRequest, RecordedOutcome,
};
pub use sqlite::{RenderStats, SqliteMetadata, TagHistoryEntry};

mod documents;
mod publication;

use crate::{
    DocumentWrite, HostedBackend, HostedDocumentForUpdate, HostedDocumentVersion, Storage,
};
use async_trait::async_trait;
use pnpr_config::MetadataBackend;
use pnpr_error::Result;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::{fmt, sync::Arc};

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
    /// The generation the view was rendered from, when the backend tracks
    /// generations.
    pub generation: Option<Generation>,
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
    /// holds it, when the backend tracks generations; `moved` says whether
    /// this change moved it, rather than finding it there.
    Committed { generation: Option<Generation>, moved: bool },
}

/// The metadata of one `(catalog, ecosystem)` namespace.
#[async_trait]
pub(crate) trait PackageMetadata: fmt::Debug + Send + Sync {
    async fn read_view(&self, name: &CanonicalPackageName) -> Result<Option<PackageView>>;

    /// Read a document with the token [`Self::write_if_current`] needs to
    /// detect a concurrent writer.
    async fn read_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>>;

    /// Commit `bytes` as the package's document while the stored one is still
    /// at `version`: absent for `None`, any for
    /// [`HostedDocumentVersion::Unversioned`].
    async fn write_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
    ) -> Result<DocumentWrite>;

    async fn change_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate>;

    /// Remove the package's metadata. Reports whether there was any. The
    /// documents backend removes a document together with the package's
    /// archives, so it removes nothing here.
    async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool>;

    /// The names of every package with metadata, sorted.
    async fn package_names(&self) -> Result<Vec<String>>;
}

/// Which metadata backend a [`Storage`] and every view of it use.
#[derive(Debug, Clone)]
pub(crate) enum MetadataSource {
    Documents,
    Sqlite(Arc<SqliteMetadata>),
}

impl MetadataSource {
    pub(crate) fn open(backend: &MetadataBackend) -> Result<Self> {
        match backend {
            MetadataBackend::Documents => Ok(Self::Documents),
            MetadataBackend::Sqlite(path) => {
                Ok(Self::Sqlite(Arc::new(SqliteMetadata::open(path)?)))
            }
        }
    }

    /// The metadata of `(org, ecosystem)`, whose archives `hosted` holds.
    pub(crate) fn view(
        &self,
        hosted: &Arc<dyn HostedBackend>,
        org: &str,
        ecosystem: Ecosystem,
    ) -> Arc<dyn PackageMetadata> {
        match self {
            Self::Documents => Arc::new(documents::DocumentMetadata::new(Arc::clone(hosted))),
            Self::Sqlite(database) => Arc::new(database.view(org, ecosystem)),
        }
    }
}

impl Storage {
    /// Read the package's document with the generation it was rendered from.
    pub async fn read_hosted_package_view(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<PackageView>> {
        self.metadata.read_view(name).await
    }

    /// Move or remove a dist-tag of a hosted package in one metadata commit.
    pub async fn change_dist_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate> {
        self.metadata.change_tag(name, change).await
    }

    /// The `SQLite` metadata database, when this storage keeps its metadata
    /// in one.
    #[must_use]
    pub fn sqlite_metadata(&self) -> Option<&Arc<SqliteMetadata>> {
        match &self.metadata_source {
            MetadataSource::Sqlite(database) => Some(database),
            MetadataSource::Documents => None,
        }
    }
}
