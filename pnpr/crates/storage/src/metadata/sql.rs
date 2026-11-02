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
//! [`sqlite::SqliteAdapter`] keeps the database in one embedded file.
//!
//! Every operation runs on the blocking pool. A write runs in one
//! transaction that holds the database's write lock from its start, so
//! commits are serialized and a compare-and-set reads the generation it
//! replaces inside its own transaction. A write the adapter refused for a
//! conflict runs again from the start. A read runs in one read transaction,
//! one committed snapshot. The render caches are shared by every read.

pub use cache::RenderStats;
pub use tags::TagHistoryEntry;

pub(crate) mod adapter;
mod batch;
mod cache;
mod commit;
mod notes;
mod packument;
mod publications;
mod publish;
mod render;
mod schema;
mod sqlite;
mod tags;
mod versions;

use super::{
    ArtifactMember, BatchDocument, Generation, PackageMetadata, PackageView, Publication,
    PublicationCommit, PublicationRequest, PublishNotes, RecordedOutcome, TagChange, TagUpdate,
};
use crate::{DocumentWrite, HostedDocumentForUpdate, HostedDocumentVersion};
use adapter::{Adapter, Connection, ReadSource, params};
use async_trait::async_trait;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::{path::Path, sync::Arc, time::Duration};

/// How many times a write runs before a conflict it keeps meeting fails it.
const WRITE_ATTEMPTS: u32 = 4;

/// The wait before the first rerun of a write that met a conflict, doubled
/// per rerun.
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(10);

/// The metadata database of one hosted store.
#[derive(Debug)]
pub struct SqlMetadata {
    adapter: Box<dyn Adapter>,
    cache: cache::RenderCache,
}

/// How current a read must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freshness {
    /// What the last commit left: a read that decides a write, or that
    /// answers a retry of one.
    Current,
    /// A view a replica may serve while it is within its freshness bound.
    Bounded,
}

impl Freshness {
    fn source(self) -> ReadSource {
        match self {
            Self::Current => ReadSource::Primary,
            Self::Bounded => ReadSource::Replica,
        }
    }
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
        let database = path.display().to_string();
        Self::with_adapter(Box::new(adapter), &database)
    }

    /// The metadata kept by `adapter`, whose schema is created here when the
    /// database is new. `database` names it in errors.
    fn with_adapter(adapter: Box<dyn Adapter>, database: &str) -> Result<Self> {
        let mut prepared = None;
        adapter.write(&mut |connection| {
            prepared = Some(schema::prepare(connection)?);
            Ok(())
        })?;
        let stored = prepared.flatten();
        if stored != Some(schema::SCHEMA_VERSION) {
            return Err(RegistryError::StorageUnavailable {
                reason: format!(
                    "the metadata database {database} uses schema {stored:?}; this pnpr reads \
                     schema {}",
                    schema::SCHEMA_VERSION,
                ),
            });
        }
        Ok(Self { adapter, cache: cache::RenderCache::default() })
    }

    /// What rendering has done since the database was opened.
    #[must_use]
    pub fn render_stats(&self) -> RenderStats {
        self.cache.stats()
    }

    /// The current generation of `catalog`, `0` before its first commit.
    pub fn catalog_generation(&self, catalog: &str) -> Result<u64> {
        self.run_read(Freshness::Current, |connection, _| catalog_generation(connection, catalog))
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
        self.run_read(Freshness::Bounded, |connection, _| tags::tag_history(connection, &key, tag))
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
        self.read(Freshness::Current, move |connection, _| {
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
        self.read(Freshness::Bounded, move |connection, _| {
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
        self.read(Freshness::Bounded, move |connection, _| {
            publications::initial_publication(connection, &key, &version)
        })
        .await
    }

    pub(super) fn view(self: &Arc<Self>, org: &str, ecosystem: Ecosystem) -> SqlView {
        SqlView { database: Arc::clone(self), catalog: org.to_string(), ecosystem }
    }

    /// Run the write `operation` on the blocking pool.
    async fn write<Output, Operation>(self: &Arc<Self>, operation: Operation) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnMut(&dyn Connection) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || database.run_write(operation)).await?
    }

    /// Run `operation` on the blocking pool in a read transaction that meets
    /// `freshness`.
    async fn read<Output, Operation>(
        self: &Arc<Self>,
        freshness: Freshness,
        operation: Operation,
    ) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnOnce(&dyn Connection, &cache::RenderCache) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || database.run_read(freshness, operation)).await?
    }

    /// Run the write `operation`, again from its start while the adapter
    /// reports a conflict it may clear.
    fn run_write<Output>(
        &self,
        mut operation: impl FnMut(&dyn Connection) -> Result<Output>,
    ) -> Result<Output> {
        let mut delay = FIRST_RETRY_DELAY;
        let mut attempt = 1;
        loop {
            let mut output = None;
            let result = self.adapter.write(&mut |connection| {
                output = Some(operation(connection)?);
                Ok(())
            });
            match (result, output) {
                (Ok(()), Some(output)) => return Ok(output),
                (Err(error), _)
                    if attempt < WRITE_ATTEMPTS && self.adapter.is_retryable(&error) =>
                {
                    std::thread::sleep(delay);
                    delay *= 2;
                    attempt += 1;
                }
                (Err(error), _) => return Err(error),
                (Ok(()), None) => unreachable!("a write that succeeded produced its output"),
            }
        }
    }

    /// Run `operation` in one read transaction that meets `freshness`.
    fn run_read<Output>(
        &self,
        freshness: Freshness,
        operation: impl FnOnce(&dyn Connection, &cache::RenderCache) -> Result<Output>,
    ) -> Result<Output> {
        let mut operation = Some(operation);
        let mut output = None;
        self.adapter.read(freshness.source(), &mut |connection| {
            let operation = operation.take().expect("an adapter runs a read once");
            connection.begin_read()?;
            let result = operation(connection, &self.cache);
            let ended =
                connection.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" });
            output = Some(result?);
            ended
        })?;
        output.ok_or_else(|| RegistryError::Internal {
            reason: "a metadata read did not run".to_string(),
        })
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

/// The metadata of one `(catalog, ecosystem)` namespace.
#[derive(Debug)]
pub(super) struct SqlView {
    database: Arc<SqlMetadata>,
    catalog: String,
    ecosystem: Ecosystem,
}

impl SqlView {
    fn key(&self, name: &CanonicalPackageName) -> PackageKey {
        PackageKey {
            catalog: self.catalog.clone(),
            ecosystem: self.ecosystem,
            name: name.as_str().to_string(),
        }
    }

    async fn read_view_with(
        &self,
        name: &CanonicalPackageName,
        freshness: Freshness,
    ) -> Result<Option<PackageView>> {
        let key = self.key(name);
        self.database.read(freshness, move |connection, cache| {
            render::read_view(connection, cache, &key)
        })
        .await
    }
}

#[async_trait]
impl PackageMetadata for SqlView {
    async fn read_view(&self, name: &CanonicalPackageName) -> Result<Option<PackageView>> {
        self.read_view_with(name, Freshness::Bounded).await
    }

    async fn read_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        let Some(view) = self.read_view_with(name, Freshness::Current).await? else {
            return Ok(None);
        };
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
        self.database.read(Freshness::Bounded, move |connection, _| {
            connection
                .query(
                    "SELECT name FROM packages WHERE catalog = ?1 AND ecosystem = ?2 ORDER BY name",
                    params![catalog, ecosystem.as_str()],
                )?
                .into_iter()
                .map(|mut row| row.get(0))
                .collect()
        })
        .await
    }
}

#[cfg(test)]
mod tests;
