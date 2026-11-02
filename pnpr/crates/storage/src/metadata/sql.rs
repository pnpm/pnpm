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
//! replaces inside its own transaction. A write the adapter refused for a
//! conflict runs again from the start. A read runs in one read transaction,
//! one committed snapshot. The render caches are shared by every read.
//!
//! A read that may lag, a [`Freshness::Bounded`] one, goes to the replica
//! when the adapter has one. It still never sees less than this process
//! committed to its catalog, nor less than a generation the caller asks for:
//! a replica found behind is synced, and read from the primary when the sync
//! does not bring it far enough.

pub(crate) mod adapter;

pub use cache::RenderStats;
pub use tags::TagHistoryEntry;

#[macro_use]
mod params;
mod batch;
mod cache;
mod commit;
#[cfg(feature = "backend-libsql")]
mod libsql;
mod notes;
mod packument;
mod publications;
mod publish;
mod render;
mod schema;
mod sqlite;
mod tags;
mod transactions;
mod versions;
mod view;

use super::{
    ArtifactMember, BatchDocument, Generation, Publication, PublicationCommit, PublicationRequest,
    PublishNotes, RecordedOutcome,
};
use crate::HostedDocumentVersion;
use adapter::{Adapter, Connection};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
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
    cache: cache::RenderCache,
    /// The latest generation this process committed in each catalog, which
    /// a replica read of that catalog must reach.
    committed: Mutex<HashMap<String, u64>>,
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
    /// Open the `SQLite` database at `path`, creating it and its directory if
    /// needed.
    ///
    /// # Errors
    ///
    /// The database cannot be opened, or was written by another pnpr layout.
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
            cache: cache::RenderCache::default(),
            committed: Mutex::new(HashMap::new()),
        }
    }

    /// Create the schema in a new database, and refuse one written by another
    /// pnpr layout.
    fn ensure_schema(&self) -> Result<()> {
        if self.schema.get().is_some() {
            return Ok(());
        }
        let mut prepared = None;
        self.adapter.write(&mut |connection| {
            prepared = Some(schema::prepare(connection)?);
            Ok(())
        })?;
        let stored = prepared.flatten();
        if stored != Some(schema::SCHEMA_VERSION) {
            return Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the metadata database {} uses schema {stored:?}; this pnpr reads schema {}",
                    self.name,
                    schema::SCHEMA_VERSION,
                ),
            });
        }
        // A replica bootstrapped before the schema existed lacks its tables.
        self.adapter.sync_replica()?;
        let _ = self.schema.set(());
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
        let catalog = catalog.to_string();
        self.write(catalog.clone(), move |connection| {
            let request = request.as_ref();
            publish::commit_publication(
                connection, &catalog, &documents, &artifacts, &notes, request,
            )
        })
        .await
    }

    /// The retained outcome of `request` in `catalog`, when an identical
    /// request committed there.
    pub async fn recorded_outcome(
        self: &Arc<Self>,
        catalog: &str,
        request: &PublicationRequest,
    ) -> Result<Option<RecordedOutcome>> {
        let (catalog, request) = (catalog.to_string(), request.clone());
        self.read(catalog.clone(), Freshness::Current, move |connection, _| {
            let PublicationRequest {
                principal,
                idempotency_key,
                request_digest,
                ..
            } = &request;
            publications::recorded_outcome(
                connection,
                &catalog,
                principal,
                idempotency_key,
                request_digest,
            )
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

    pub(super) fn view(self: &Arc<Self>, org: &str, ecosystem: Ecosystem) -> view::SqlView {
        view::SqlView { database: Arc::clone(self), catalog: org.to_string(), ecosystem }
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
