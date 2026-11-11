//! Metadata in a SQL database.
//!
//! Package documents are stored as normalized rows per ecosystem — accepted
//! version manifests, READMEs, tags, deprecations, crate versions, Python
//! distribution files, OCI manifests and tags, plus package-level state
//! versioned by `packages.state_version` — and rendered through the fragment
//! and view caches in [`render`].

#[cfg(feature = "backend-libsql")]
pub(crate) use pnpr_sql::libsql;
#[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) use pnpr_sql::network;
pub(crate) use pnpr_sql::{adapter, schema, sqlite};

pub use cache::RenderStats;
pub use cdn::CdnInvalidation;
pub use mappings::{Mapping, Unmapping};
pub use rule_overrides::{
    RuleOverridesWrite, StoredPackageOverride, StoredRuleOverrideSet, StoredRuleOverrides,
};
pub use scim::{NewScimUser, ScimStatus, ScimUserRemoval, ScimUserWrite, StoredScimUser};
pub(crate) use staged::{StagedEntry, StagedId};
pub use tags::TagHistoryEntry;
pub use team_rosters::{StoredTeam, StoredTeamRoster, TeamRosterWrite};
pub(crate) use view::PackageMetadata;

mod batch;
mod blobs;
mod cache;
mod cargo_rows;
mod cdn;
mod commit;
mod fence;
mod log;
mod mappings;
mod notes;
mod oci_rows;
mod packument;
mod publications;
mod publish;
mod pypi_rows;
mod reclaims;
mod records;
mod removed;
mod render;
mod rule_overrides;
mod schema_monitor;
mod scim;
mod staged;
mod tags;
mod team_rosters;
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
    schema_monitor: schema_monitor::SchemaMonitor,
    cache: cache::RenderCache,
}

/// Which package a statement addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PackageKey {
    namespace: String,
    ecosystem: Ecosystem,
    name: String,
}

impl PackageKey {
    fn params(&self) -> (&str, &str, &str) {
        (&self.namespace, self.ecosystem.as_str(), &self.name)
    }

    fn generation(&self, value: u64) -> Generation {
        Generation { namespace: self.namespace.clone(), value }
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
    #[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
    pub(crate) fn open_network(settings: &pnpr_config::SqlBackendSettings) -> Result<Self> {
        Ok(Self::with_adapter(
            network::adapter(settings)?,
            pnpr_config::redact_connection_url(&settings.url),
        ))
    }

    /// The version of the schema this pnpr creates and reads. A database
    /// that holds a newer or pre-1.0 schema is refused.
    pub const SCHEMA_VERSION: i64 = schema::SCHEMA_VERSION;

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
        let name = pnpr_config::redact_connection_url(&settings.url);
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
            schema_monitor: schema_monitor::SchemaMonitor::default(),
            cache: cache::RenderCache::default(),
        }
    }

    /// Create the schema in an empty database, and apply pending numbered
    /// migration steps to one at an older version. Refuse a database from a
    /// newer release, and a pre-1.0 database without `schema_migrations`;
    /// the operator wipes pnpr's tables, and pnpr creates them afresh.
    fn ensure_schema(&self) -> Result<()> {
        if self.schema.get().is_some() {
            self.schema_monitor.result()?;
            if self.schema_monitor.claim() {
                self.schema_monitor.probe(&*self.adapter, &self.name);
            }
            return self.schema_monitor.result();
        }
        let _initializing = self.schema_check.lock().unwrap_or_else(PoisonError::into_inner);
        if self.schema.get().is_some() {
            return self.schema_monitor.result();
        }
        if let Some(stored) = schema::prepare_adapter_with_steps(&*self.adapter, schema::STEPS)? {
            return Err(schema::refused_error(&self.name, &stored));
        }
        self.schema_monitor.checked();
        let _ = self.schema.set(());
        // A replica bootstrapped before the schema existed lacks its tables.
        // One that cannot sync now is left stale, and its reads sync first or
        // go to the primary.
        if let Err(error) = self.adapter.sync_replica(Instant::now()) {
            tracing::warn!(%error, "the metadata replica could not sync; reads use the primary");
        }
        Ok(())
    }

    /// Refuse a known incompatible schema and schedule at most one primary probe.
    /// Requests retain the last compatible result while that probe is pending.
    pub async fn check_schema(self: &Arc<Self>) -> Result<()> {
        self.schema_monitor.result()?;
        if self.schema.get().is_none() {
            let metadata = Arc::clone(self);
            return tokio::task::spawn_blocking(move || metadata.ensure_schema()).await?;
        }
        if self.schema_monitor.claim() {
            let metadata = Arc::clone(self);
            tokio::task::spawn_blocking(move || {
                metadata.schema_monitor.probe(&*metadata.adapter, &metadata.name);
            });
        }
        self.schema_monitor.result()
    }

    /// What rendering has done since the database was opened.
    #[must_use]
    pub fn render_stats(&self) -> RenderStats {
        self.cache.stats()
    }

    /// The current generation of `namespace`, `0` before its first commit.
    pub async fn namespace_generation(self: &Arc<Self>, namespace: &str) -> Result<u64> {
        let namespace = namespace.to_string();
        self.read(namespace.clone(), Freshness::Current, move |connection, _| {
            namespace_generation(connection, &namespace)
        })
        .await
    }

    /// Every recorded move of `tag`, oldest first, including moves recorded
    /// before the package was removed.
    pub async fn tag_history(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
        tag: &str,
    ) -> Result<Vec<TagHistoryEntry>> {
        let key = PackageKey {
            namespace: namespace.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        let tag = tag.to_string();
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            tags::tag_history(connection, &key, &tag)
        })
        .await
    }

    /// Commit `documents` of `namespace` as one publication, under one new
    /// generation, or none of them. `artifacts` are the members of other
    /// ecosystems than npm, whose versions are recorded from the documents.
    pub async fn commit_publication(
        self: &Arc<Self>,
        namespace: &str,
        documents: Vec<BatchDocument>,
        artifacts: Vec<ArtifactMember>,
        notes: PublishNotes,
        request: Option<PublicationRequest>,
    ) -> Result<PublicationCommit> {
        let fence = BlobFence::default();
        self.commit_fenced_publication(namespace, documents, artifacts, notes, request, fence).await
    }

    /// [`Self::commit_publication`], mapping every file of `fence` to its
    /// blob, and refused with [`RegistryError::ArchiveReclaimed`] unless
    /// each blob's incarnation is live or is the next one, which the commit
    /// creates (see [`crate::metadata::BlobRow`]). The check is read with the
    /// commit's other reads, in the same round trip, and the rows are written
    /// with the commit.
    pub async fn commit_fenced_publication(
        self: &Arc<Self>,
        namespace: &str,
        documents: Vec<BatchDocument>,
        artifacts: Vec<ArtifactMember>,
        notes: PublishNotes,
        request: Option<PublicationRequest>,
        fence: BlobFence,
    ) -> Result<PublicationCommit> {
        let namespace = namespace.to_string();
        self.write(move |connection| {
            let submission = publish::Submission {
                namespace: &namespace,
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

    /// The retained outcome of `request` in `namespace`, when an identical
    /// request committed there and none of its npm versions was unpublished.
    pub async fn recorded_outcome(
        self: &Arc<Self>,
        namespace: &str,
        request: &PublicationRequest,
    ) -> Result<Option<RecordedOutcome>> {
        let (namespace, request) = (namespace.to_string(), request.clone());
        self.read(namespace.clone(), Freshness::Current, move |connection, _| {
            publications::recorded_outcome(connection, &namespace, &request)
        })
        .await
    }

    /// The publication of `namespace` with this id or name.
    pub async fn publication(
        self: &Arc<Self>,
        namespace: &str,
        id_or_name: &str,
    ) -> Result<Option<Publication>> {
        let (namespace, id_or_name) = (namespace.to_string(), id_or_name.to_string());
        self.read_present(move |connection, _| {
            publications::publication(connection, &namespace, &id_or_name)
        })
        .await
    }

    /// The publication that introduced `version` of `name` in `namespace`.
    pub async fn initial_publication(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
        version: &str,
    ) -> Result<Option<Publication>> {
        let key = PackageKey {
            namespace: namespace.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        let version = version.to_string();
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            publications::initial_publication(connection, &key, &version)
        })
        .await
    }

    /// Every transparency log with a sequenced entry, with its size.
    pub async fn sequenced_logs(self: &Arc<Self>) -> Result<Vec<(LogId, u64)>> {
        // Any namespace: the read sees every log.
        self.read(String::new(), Freshness::Current, move |connection, _| log::sizes(connection))
            .await
    }

    /// The number of entries sequenced in `log`.
    pub async fn log_size(self: &Arc<Self>, log: &LogId) -> Result<u64> {
        let log = log.clone();
        self.read(log.namespace.clone(), Freshness::Current, move |connection, _| {
            log::size(connection, &log)
        })
        .await
    }

    /// The tree `log` was integrated to, its size and root, `None` before
    /// its first integration.
    pub async fn log_head(self: &Arc<Self>, log: &LogId) -> Result<Option<(u64, Vec<u8>)>> {
        let log = log.clone();
        self.read(log.namespace.clone(), Freshness::Current, move |connection, _| {
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
        self.read(log.namespace.clone(), Freshness::Current, move |connection, _| {
            log::entries(connection, &log, indexes.clone())
        })
        .await
    }

    /// The index of the log entry of the release record `version` of `name`
    /// in `namespace` was accepted with, and the record's digest.
    pub async fn release_record_entry(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
        version: &str,
    ) -> Result<Option<(u64, String)>> {
        let key = PackageKey {
            namespace: namespace.to_string(),
            ecosystem: name.ecosystem(),
            name: name.as_str().to_string(),
        };
        let version = version.to_string();
        self.read_present(move |connection, _| log::record_entry(connection, &key, &version)).await
    }

    /// The metadata of `(namespace, ecosystem)`.
    pub(crate) fn view(self: &Arc<Self>, namespace: &str, ecosystem: Ecosystem) -> PackageMetadata {
        PackageMetadata { database: Arc::clone(self), namespace: namespace.to_string(), ecosystem }
    }
}

/// The current generation of `namespace`, `0` before its first commit.
fn namespace_generation(connection: &dyn Connection, namespace: &str) -> Result<u64> {
    let row = connection.query_optional(
        "SELECT generation FROM namespaces WHERE namespace = ?1",
        params![namespace],
    )?;
    row.map_or(Ok(0), |mut row| Ok(to_generation(row.get(0)?)))
}

/// Record `generation` as `namespace`'s current one.
fn advance_namespace(connection: &dyn Connection, namespace: &str, generation: u64) -> Result<()> {
    connection.execute(
        "INSERT INTO namespaces (namespace, generation) VALUES (?1, ?2)
         ON CONFLICT (namespace) DO UPDATE SET generation = excluded.generation",
        params![namespace, to_column(generation)],
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
