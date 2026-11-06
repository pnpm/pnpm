//! Metadata in a SQL database.
//!
//! An npm packument is stored as rows: one per accepted version manifest,
//! README, tag, deprecation, and the package-level fields. A version's rows
//! are insert-only; tags, deprecations, and package fields change without
//! touching them. Another ecosystem's document is stored whole. Reads render
//! the packument from those rows through the fragment cache in [`render`].
//!
//! The schema, the queries, and the rules they enforce are shared by every
//! database. What differs between databases is behind [`adapter::Adapter`]:
//! [`sqlite::SqliteAdapter`] keeps the database in one embedded file, and
//! `libsql::LibsqlAdapter` in a libSQL primary, such as Turso, with an
//! optional embedded replica.
//!
//! Every operation runs on the blocking pool. A write runs in one
//! transaction that holds the database's write lock from its start, so
//! commits are serialized and a compare-and-set reads the generation it
//! replaces inside its own transaction. A write reads everything it compares
//! with in one [`adapter::Connection::query_all`] as it begins, and decides
//! in Rust, so a remote database answers it in one round trip and takes the
//! writes with the commit in another. A write the adapter refused for a
//! conflict runs again from the start. A read runs in one read transaction,
//! one committed snapshot. The render caches are shared by every read.
//!
//! A read that may lag, a [`Freshness::Bounded`] one, goes to the replica
//! when the adapter has one. A [`Freshness::AtLeast`] read there first checks
//! the catalog's generation: a replica found behind is synced, and read from
//! the primary when the sync does not bring it far enough.

pub(crate) mod adapter;

pub use cache::RenderStats;
pub use mappings::{Mapping, Unmapping};
pub(crate) use staged::{StagedEntry, StagedId};
pub use tags::TagHistoryEntry;
pub(crate) use view::PackageMetadata;

#[macro_use]
mod params;
mod batch;
mod blobs;
mod cache;
mod commit;
mod fence;
#[cfg(feature = "backend-libsql")]
mod libsql;
mod log;
mod mappings;
mod notes;
mod packument;
mod publications;
mod publish;
mod reclaims;
mod render;
mod schema;
mod sqlite;
mod staged;
mod tags;
mod transactions;
mod versions;
mod view;

use super::{
    ArtifactMember, BatchDocument, BlobFence, Generation, Publication, PublicationCommit,
    PublicationRequest, PublishNotes, RecordedOutcome,
};
use crate::{HostedDocumentVersion, transparency_log::LogId};
use adapter::{Adapter, Connection};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Instant,
};
use transactions::Freshness;

/// The metadata database of one hosted store.
#[derive(Debug)]
pub struct SqlMetadata {
    adapter: Box<dyn Adapter>,
    /// The database, as errors name it.
    name: String,
    /// Set once the schema is known to be the one this pnpr reads.
    schema: OnceLock<()>,
    /// Held while the schema is checked, so concurrent first operations
    /// check it once.
    schema_check: Mutex<()>,
    cache: cache::RenderCache,
}

/// Which package a statement addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PackageKey {
    catalog: String,
    ecosystem: Ecosystem,
    name: String,
}

impl PackageKey {
    fn params(&self) -> (&str, &str, &str) {
        (&self.catalog, self.ecosystem.as_str(), &self.name)
    }

    fn generation(&self, value: u64) -> Generation {
        Generation { catalog: self.catalog.clone(), value }
    }
}

/// The stored document a write expects to replace.
#[derive(Debug, Clone, Copy)]
enum Expected {
    Absent,
    Generation(u64),
    Any,
}

impl Expected {
    fn from_version(version: Option<&HostedDocumentVersion>) -> Result<Self> {
        match version {
            None => Ok(Self::Absent),
            Some(HostedDocumentVersion::Generation(value)) => Ok(Self::Generation(*value)),
            Some(HostedDocumentVersion::Unversioned) => Ok(Self::Any),
            Some(HostedDocumentVersion::ObjectVersion(_)) => Err(RegistryError::Internal {
                reason: "an object-store document version reached the SQL metadata store"
                    .to_string(),
            }),
        }
    }

    fn admits(self, current: Option<u64>) -> bool {
        match self {
            Self::Absent => current.is_none(),
            Self::Generation(expected) => current == Some(expected),
            Self::Any => true,
        }
    }
}

impl SqlMetadata {
    /// The version of the schema this pnpr creates and reads. A database
    /// that holds another one is refused.
    pub const SCHEMA_VERSION: &'static str = schema::SCHEMA_VERSION;

    /// The statements that create the schema, so a cache of databases can
    /// tell a layout that changed under one version apart.
    pub const SCHEMA_TABLES: &'static str = schema::TABLES;

    /// Open the `SQLite` database at `path`, creating it and its directory if
    /// needed.
    ///
    /// # Errors
    ///
    /// The database cannot be opened, or holds another metadata schema.
    pub fn open_sqlite(path: &Path) -> Result<Self> {
        let adapter = sqlite::SqliteAdapter::open(path)?;
        let metadata = Self::with_adapter(Box::new(adapter), path.display().to_string());
        metadata.ensure_schema()?;
        Ok(metadata)
    }

    /// The libSQL database `settings` name. It opens on first use.
    #[cfg(feature = "backend-libsql")]
    #[must_use]
    pub fn open_libsql(settings: &pnpr_config::LibsqlSettings) -> Self {
        let name = format!("{settings:?}");
        Self::with_adapter(Box::new(libsql::LibsqlAdapter::new(settings.clone())), name)
    }

    /// The metadata kept by `adapter`. The schema is checked, and created in
    /// a new database, before the first operation. `name` names the
    /// database in errors.
    fn with_adapter(adapter: Box<dyn Adapter>, name: String) -> Self {
        Self {
            adapter,
            name,
            schema: OnceLock::new(),
            schema_check: Mutex::new(()),
            cache: cache::RenderCache::default(),
        }
    }

    /// Create the schema in an empty database, and refuse one that holds
    /// another schema. pnpr does not migrate the schema before 1.0: the
    /// operator wipes pnpr's metadata tables, and pnpr creates them afresh.
    fn ensure_schema(&self) -> Result<()> {
        if self.schema.get().is_some() {
            return Ok(());
        }
        let _check = self.schema_check.lock().unwrap_or_else(PoisonError::into_inner);
        if self.schema.get().is_some() {
            return Ok(());
        }
        let mut refused = None;
        self.retry_conflicts(|| {
            self.adapter.write(&mut |connection| {
                refused = schema::prepare(connection)?;
                Ok(())
            })
        })?;
        if let Some(stored) = refused {
            return Err(RegistryError::Internal {
                reason: format!(
                    "the metadata schema changed: the metadata database {} holds {stored}, and \
                     this pnpr expects schema version {}. pnpr does not migrate its metadata \
                     before 1.0; wipe pnpr's metadata tables, and pnpr creates them afresh",
                    self.name,
                    schema::SCHEMA_VERSION,
                ),
            });
        }
        let _ = self.schema.set(());
        // A replica bootstrapped before the schema existed lacks its tables.
        // One that cannot sync now is left stale, and its reads sync first or
        // go to the primary.
        if let Err(error) = self.adapter.sync_replica(Instant::now()) {
            tracing::warn!(%error, "the metadata replica could not sync; reads use the primary");
        }
        Ok(())
    }

    /// What rendering has done since the database was opened.
    #[must_use]
    pub fn render_stats(&self) -> RenderStats {
        self.cache.stats()
    }

    /// The current generation of `catalog`, `0` before its first commit.
    pub async fn catalog_generation(self: &Arc<Self>, catalog: &str) -> Result<u64> {
        let catalog = catalog.to_string();
        self.read(catalog.clone(), Freshness::Current, move |connection, _| {
            catalog_generation(connection, &catalog)
        })
        .await
    }

    /// Every recorded move of `tag`, oldest first, including moves recorded
    /// before the package was removed.
    pub async fn tag_history(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        tag: &str,
    ) -> Result<Vec<TagHistoryEntry>> {
        let key = PackageKey {
            catalog: org.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        let tag = tag.to_string();
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            tags::tag_history(connection, &key, &tag)
        })
        .await
    }

    /// Commit `documents` of `catalog` as one publication, under one new
    /// generation, or none of them. `artifacts` are the members of other
    /// ecosystems than npm, whose versions are recorded from the documents.
    pub async fn commit_publication(
        self: &Arc<Self>,
        catalog: &str,
        documents: Vec<BatchDocument>,
        artifacts: Vec<ArtifactMember>,
        notes: PublishNotes,
        request: Option<PublicationRequest>,
    ) -> Result<PublicationCommit> {
        let fence = BlobFence::default();
        self.commit_fenced_publication(catalog, documents, artifacts, notes, request, fence).await
    }

    /// [`Self::commit_publication`], mapping every file of `fence` to its
    /// blob, and refused with [`RegistryError::ArchiveReclaimed`] unless
    /// each blob's incarnation is live or is the next one, which the commit
    /// creates (see [`crate::metadata::BlobRow`]). The check is read with the
    /// commit's other reads, in the same round trip, and the rows are written
    /// with the commit.
    pub async fn commit_fenced_publication(
        self: &Arc<Self>,
        catalog: &str,
        documents: Vec<BatchDocument>,
        artifacts: Vec<ArtifactMember>,
        notes: PublishNotes,
        request: Option<PublicationRequest>,
        fence: BlobFence,
    ) -> Result<PublicationCommit> {
        let catalog = catalog.to_string();
        self.write(move |connection| {
            let submission = publish::Submission {
                catalog: &catalog,
                documents: &documents,
                artifacts: &artifacts,
                notes: &notes,
                request: request.as_ref(),
                fence: &fence,
            };
            publish::commit_publication(connection, &submission)
        })
        .await
    }

    /// The retained outcome of `request` in `catalog`, when an identical
    /// request committed there and none of its npm versions was unpublished.
    pub async fn recorded_outcome(
        self: &Arc<Self>,
        catalog: &str,
        request: &PublicationRequest,
    ) -> Result<Option<RecordedOutcome>> {
        let (catalog, request) = (catalog.to_string(), request.clone());
        self.read(catalog.clone(), Freshness::Current, move |connection, _| {
            publications::recorded_outcome(connection, &catalog, &request)
        })
        .await
    }

    /// The publication of `catalog` with this id or name.
    pub async fn publication(
        self: &Arc<Self>,
        catalog: &str,
        id_or_name: &str,
    ) -> Result<Option<Publication>> {
        let (catalog, id_or_name) = (catalog.to_string(), id_or_name.to_string());
        self.read(catalog.clone(), Freshness::Bounded, move |connection, _| {
            publications::publication(connection, &catalog, &id_or_name)
        })
        .await
    }

    /// The publication that introduced `version` of `name` in `org`.
    pub async fn initial_publication(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
    ) -> Result<Option<Publication>> {
        let key = PackageKey {
            catalog: org.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        let version = version.to_string();
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            publications::initial_publication(connection, &key, &version)
        })
        .await
    }

    /// Every transparency log with a sequenced entry, with its size.
    pub async fn sequenced_logs(self: &Arc<Self>) -> Result<Vec<(LogId, u64)>> {
        // Any catalog: the read sees every log.
        self.read(String::new(), Freshness::Current, move |connection, _| log::sizes(connection))
            .await
    }

    /// The number of entries sequenced in `log`.
    pub async fn log_size(self: &Arc<Self>, log: &LogId) -> Result<u64> {
        let log = log.clone();
        self.read(log.catalog.clone(), Freshness::Current, move |connection, _| {
            log::size(connection, &log)
        })
        .await
    }

    /// The tree `log` was integrated to, its size and root, `None` before
    /// its first integration.
    pub async fn log_head(self: &Arc<Self>, log: &LogId) -> Result<Option<(u64, Vec<u8>)>> {
        let log = log.clone();
        self.read(log.catalog.clone(), Freshness::Current, move |connection, _| {
            log::head(connection, &log)
        })
        .await
    }

    /// Record that `log` was integrated to the tree of `size` entries with
    /// `root`, unless it already was to a larger one.
    pub async fn advance_log_head(
        self: &Arc<Self>,
        log: &LogId,
        size: u64,
        root: [u8; 32],
    ) -> Result<()> {
        let log = log.clone();
        self.write(move |connection| log::advance_head(connection, &log, size, &root)).await
    }

    /// The sequenced entries of `log` at `indexes`, in order.
    pub async fn log_entries(
        self: &Arc<Self>,
        log: &LogId,
        indexes: std::ops::Range<u64>,
    ) -> Result<Vec<Vec<u8>>> {
        let log = log.clone();
        self.read(log.catalog.clone(), Freshness::Current, move |connection, _| {
            log::entries(connection, &log, indexes.clone())
        })
        .await
    }

    /// The index of the log entry of the release record `version` of `name`
    /// in `org` was accepted with, and the record's digest.
    pub async fn release_record_entry(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
    ) -> Result<Option<(u64, String)>> {
        let key = PackageKey {
            catalog: org.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        let version = version.to_string();
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            log::record_entry(connection, &key, &version)
        })
        .await
    }

    /// The metadata of `(org, ecosystem)`.
    pub(crate) fn view(self: &Arc<Self>, org: &str, ecosystem: Ecosystem) -> PackageMetadata {
        PackageMetadata { database: Arc::clone(self), catalog: org.to_string(), ecosystem }
    }
}

/// The current generation of `catalog`, `0` before its first commit.
fn catalog_generation(connection: &dyn Connection, catalog: &str) -> Result<u64> {
    let row = connection.query_optional(
        "SELECT generation FROM catalogs WHERE catalog = ?1",
        params![catalog],
    )?;
    row.map_or(Ok(0), |mut row| Ok(to_generation(row.get(0)?)))
}

/// Record `generation` as `catalog`'s current one.
fn advance_catalog(connection: &dyn Connection, catalog: &str, generation: u64) -> Result<()> {
    connection.execute(
        "INSERT INTO catalogs (catalog, generation) VALUES (?1, ?2)
         ON CONFLICT (catalog) DO UPDATE SET generation = excluded.generation",
        params![catalog, to_column(generation)],
    )?;
    Ok(())
}

/// A stored generation. Generations start at `1` and only grow.
fn to_generation(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn to_column(generation: u64) -> i64 {
    i64::try_from(generation).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
