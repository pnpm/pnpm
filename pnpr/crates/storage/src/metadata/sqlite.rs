//! Metadata in an embedded `SQLite` database.
//!
//! An npm packument is stored as rows: one per accepted version manifest,
//! README, tag, deprecation, and the package-level fields. A version's rows
//! are insert-only; tags, deprecations, and package fields change without
//! touching them. Another ecosystem's document is stored whole. Reads render
//! the packument from those rows through the fragment cache in [`render`].
//!
//! Every operation runs on the blocking pool. Writes share one connection
//! behind a mutex and take `BEGIN IMMEDIATE`, so commits are serialized and a
//! compare-and-set reads the generation it replaces inside its own
//! transaction. Reads use connections of their own, so a read never waits for
//! a commit or for another read's render: in WAL mode each read transaction
//! sees one committed snapshot while a write is in progress. The render
//! caches are shared by every read connection.

pub use cache::RenderStats;
pub use tags::TagHistoryEntry;

mod batch;
mod cache;
mod commit;
mod notes;
mod packument;
mod publications;
mod publish;
mod render;
mod schema;
mod tags;
mod versions;

use super::{
    ArtifactMember, BatchDocument, Generation, PackageMetadata, PackageView, Publication,
    PublicationCommit, PublicationRequest, PublishNotes, RecordedOutcome, TagChange, TagUpdate,
};
use crate::{DocumentWrite, HostedDocumentForUpdate, HostedDocumentVersion};
use async_trait::async_trait;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use rusqlite::{Connection, OptionalExtension, Transaction};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

/// Read connections kept open between reads. More concurrent reads open
/// more, and close them when they are done.
const IDLE_READERS: usize = 8;

/// The metadata database of one hosted store.
#[derive(Debug)]
pub struct SqliteMetadata {
    path: PathBuf,
    /// The connection every write commits through.
    writer: Mutex<Connection>,
    /// Read connections not in use.
    readers: Mutex<Vec<Connection>>,
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
                reason: "an object-store document version reached the SQLite metadata store"
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

impl SqliteMetadata {
    /// Open the database at `path`, creating it and its directory if needed.
    ///
    /// # Errors
    ///
    /// The database cannot be opened, or was written by another pnpr layout.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        schema::prepare(&connection)?;
        let stored = schema::stored_version(&connection)?;
        if stored != Some(schema::SCHEMA_VERSION) {
            return Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the metadata database {} uses schema {stored:?}; this pnpr reads schema {}",
                    path.display(),
                    schema::SCHEMA_VERSION,
                ),
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            writer: Mutex::new(connection),
            readers: Mutex::new(Vec::new()),
            cache: cache::RenderCache::default(),
        })
    }

    /// What rendering has done since the database was opened.
    #[must_use]
    pub fn render_stats(&self) -> RenderStats {
        self.cache.stats()
    }

    /// The current generation of `catalog`, `0` before its first commit.
    pub fn catalog_generation(&self, catalog: &str) -> Result<u64> {
        self.with_reader(|connection| Ok(catalog_generation(connection, catalog)?))
    }

    /// Every recorded move of `tag`, oldest first, including moves recorded
    /// before the package was removed.
    pub fn tag_history(
        &self,
        name: &CanonicalPackageName,
        org: &str,
        tag: &str,
    ) -> Result<Vec<TagHistoryEntry>> {
        let key = PackageKey {
            catalog: org.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        self.with_reader(|connection| tags::tag_history(connection, &key, tag))
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
        self.write(move |connection| {
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
        self.read(move |connection, _| {
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
        self.read(move |connection, _| publications::publication(connection, &catalog, &id_or_name))
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
        self.read(move |connection, _| {
            publications::initial_publication(connection, &key, &version)
        })
        .await
    }

    pub(super) fn view(self: &Arc<Self>, org: &str, ecosystem: Ecosystem) -> SqliteView {
        SqliteView { database: Arc::clone(self), catalog: org.to_string(), ecosystem }
    }

    /// Run `operation` on the blocking pool with the write connection.
    async fn write<Output, Operation>(self: &Arc<Self>, operation: Operation) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnOnce(&mut Connection) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || operation(&mut lock(&database.writer))).await?
    }

    /// Run `operation` on the blocking pool with a read connection.
    async fn read<Output, Operation>(self: &Arc<Self>, operation: Operation) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnOnce(&mut Connection, &cache::RenderCache) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            database.with_reader(|connection| operation(connection, &database.cache))
        })
        .await?
    }

    /// Run `operation` with a read connection: an idle one, or a new one when
    /// every one is in use.
    fn with_reader<Output>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<Output>,
    ) -> Result<Output> {
        let idle = lock(&self.readers).pop();
        let mut connection = match idle {
            Some(connection) => connection,
            None => schema::open_reader(&self.path)?,
        };
        let output = operation(&mut connection);
        let mut readers = lock(&self.readers);
        if readers.len() < IDLE_READERS {
            readers.push(connection);
        }
        output
    }
}

fn lock<Value>(mutex: &Mutex<Value>) -> MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The current generation of `catalog`, `0` before its first commit.
fn catalog_generation(connection: &Connection, catalog: &str) -> rusqlite::Result<u64> {
    let value: Option<i64> = connection
        .query_row("SELECT generation FROM catalogs WHERE catalog = ?1", [catalog], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(value.map_or(0, to_generation))
}

/// Record `generation` as `catalog`'s current one.
fn advance_catalog(transaction: &Transaction<'_>, catalog: &str, generation: u64) -> Result<()> {
    transaction.execute(
        "INSERT INTO catalogs (catalog, generation) VALUES (?1, ?2)
         ON CONFLICT (catalog) DO UPDATE SET generation = excluded.generation",
        rusqlite::params![catalog, to_column(generation)],
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

/// The metadata of one `(catalog, ecosystem)` namespace.
#[derive(Debug)]
pub(super) struct SqliteView {
    database: Arc<SqliteMetadata>,
    catalog: String,
    ecosystem: Ecosystem,
}

impl SqliteView {
    fn key(&self, name: &CanonicalPackageName) -> PackageKey {
        PackageKey {
            catalog: self.catalog.clone(),
            ecosystem: self.ecosystem,
            name: name.as_str().to_string(),
        }
    }
}

#[async_trait]
impl PackageMetadata for SqliteView {
    async fn read_view(&self, name: &CanonicalPackageName) -> Result<Option<PackageView>> {
        let key = self.key(name);
        self.database.read(move |connection, cache| render::read_view(connection, cache, &key))
            .await
    }

    async fn read_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        let Some(view) = self.read_view(name).await? else { return Ok(None) };
        let generation = view.generation.map_or(0, |generation| generation.value);
        Ok(Some(HostedDocumentForUpdate {
            bytes: view.bytes.to_vec(),
            version: HostedDocumentVersion::Generation(generation),
        }))
    }

    async fn write_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
        principal: Option<&str>,
    ) -> Result<DocumentWrite> {
        let expected = Expected::from_version(version)?;
        let key = self.key(name);
        let bytes = bytes.to_vec();
        let principal = principal.map(str::to_string);
        self.database.write(move |connection| {
            let principal = principal.as_deref();
            commit::write_document(connection, &key, &bytes, expected, principal)
        })
        .await
    }

    async fn change_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate> {
        let key = self.key(name);
        let change = tags::OwnedTagChange::from(change);
        self.database.write(move |connection| tags::change_tag(connection, &key, &change)).await
    }

    async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        let key = self.key(name);
        self.database.write(move |connection| commit::remove_package(connection, &key)).await
    }

    async fn package_names(&self) -> Result<Vec<String>> {
        let (catalog, ecosystem) = (self.catalog.clone(), self.ecosystem);
        self.database.read(move |connection, _| {
            let mut statement = connection.prepare_cached(
                "SELECT name FROM packages WHERE catalog = ?1 AND ecosystem = ?2 ORDER BY name",
            )?;
            let names = statement
                .query_map((&catalog, ecosystem.as_str()), |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(names)
        })
        .await
    }
}

#[cfg(test)]
mod tests;
