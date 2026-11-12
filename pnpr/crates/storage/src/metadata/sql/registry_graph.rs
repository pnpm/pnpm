//! Revisions of the registry graph (`registry_graph_revisions`). The newest
//! revision is the current graph; older ones stay readable for history and
//! restore until pruning drops them.

use super::{
    adapter::{Connection, Row},
    batch::WriteTransaction,
    cdn, to_column, to_generation,
    transactions::Freshness,
};
use crate::{Storage, publish::now_millis};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use pnpr_sql::DialectSql;

/// How many revisions a write keeps, the one it writes included.
pub const REGISTRY_GRAPH_REVISIONS_KEPT: u64 = 500;

/// One stored revision of the registry graph, with its document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRegistryGraph {
    pub generation: u64,
    pub document: String,
    pub actor_id: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// The generation whose document this revision restored.
    pub restored_from: Option<u64>,
}

/// One stored revision of the registry graph, without its document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryGraphRevision {
    pub generation: u64,
    pub actor_id: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// The generation whose document this revision restored.
    pub restored_from: Option<u64>,
}

/// The outcome of [`Storage::write_registry_graph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryGraphWrite {
    Written(u64),
    /// Another writer stored a revision since the caller's read. `current` is
    /// the stored generation, `None` when no graph is stored.
    Conflict {
        current: Option<u64>,
    },
    /// A namespace the revision vacates holds a package, so nothing was
    /// written.
    HostedNotEmpty(HostedNamespace),
}

/// A hosted storage namespace of one ecosystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedNamespace {
    pub namespace: String,
    pub ecosystem: Ecosystem,
}

/// A revision for [`Storage::write_registry_graph`].
#[derive(Debug, Clone, Copy)]
pub struct NewRegistryGraph<'a> {
    pub document: &'a str,
    pub actor_id: &'a str,
    /// The generation whose document this revision restores.
    pub restored_from: Option<u64>,
    /// The hosted namespaces the revision stops serving, which must hold no
    /// package when it is written.
    pub vacated: &'a [HostedNamespace],
}

/// Lock a namespace's row the way a publish into it does, so a publish and a
/// write that vacates the namespace take turns.
const LOCK_NAMESPACE: &str = "UPDATE namespaces SET generation = generation WHERE namespace = ?1";

const NAMESPACE_HAS_PACKAGES: DialectSql = DialectSql::with_mysql(
    "SELECT 1 FROM packages WHERE namespace = ?1 AND ecosystem = ?2 LIMIT 1",
    "SELECT 1 FROM packages WHERE namespace = ?1 AND ecosystem = ?2 LIMIT 1 LOCK IN SHARE MODE",
);

const REVISION_COLUMNS: &str = "generation, document, actor_id, created_at, restored_from";

impl Storage {
    /// The generation of the current registry graph, `None` when none is
    /// stored.
    pub async fn read_registry_graph_generation(&self) -> Result<Option<u64>> {
        self.database.read(String::new(), Freshness::Current, |connection, _| {
            current_generation(connection)
        })
        .await
    }

    /// The current registry graph, `None` when none is stored.
    pub async fn read_registry_graph(&self) -> Result<Option<StoredRegistryGraph>> {
        self.database.read(String::new(), Freshness::Current, |connection, _| {
            connection
                .query_optional(
                    &format!(
                        "SELECT {REVISION_COLUMNS} FROM registry_graph_revisions
                         ORDER BY generation DESC LIMIT 1",
                    ),
                    params![],
                )?
                .map(stored_graph)
                .transpose()
        })
        .await
    }

    /// The revision with `generation`, `None` when it was never stored or was
    /// pruned.
    pub async fn read_registry_graph_revision(
        &self,
        generation: u64,
    ) -> Result<Option<StoredRegistryGraph>> {
        let generation = to_column(generation);
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query_optional(
                    &format!(
                        "SELECT {REVISION_COLUMNS} FROM registry_graph_revisions
                         WHERE generation = ?1",
                    ),
                    params![generation],
                )?
                .map(stored_graph)
                .transpose()
        })
        .await
    }

    /// Up to `limit` revisions older than `before` (every revision when
    /// `None`), newest first.
    pub async fn list_registry_graph_revisions(
        &self,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<RegistryGraphRevision>> {
        let before = before.map_or(i64::MAX, to_column);
        let limit = i64::from(limit);
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query(
                    "SELECT generation, actor_id, created_at, restored_from
                     FROM registry_graph_revisions WHERE generation < ?1
                     ORDER BY generation DESC LIMIT ?2",
                    params![before, limit],
                )?
                .into_iter()
                .map(revision_summary)
                .collect()
        })
        .await
    }

    /// Store `revision` as the current registry graph, if the current
    /// generation is still `expected` (`None`: no graph is stored) and every
    /// namespace it vacates is empty. The write invalidates every CDN-cached
    /// response and keeps the newest [`REGISTRY_GRAPH_REVISIONS_KEPT`]
    /// revisions.
    pub async fn write_registry_graph(
        &self,
        expected: Option<u64>,
        revision: NewRegistryGraph<'_>,
    ) -> Result<RegistryGraphWrite> {
        let document = revision.document.to_string();
        let actor_id = revision.actor_id.to_string();
        let restored_from = revision.restored_from.map(to_column);
        let vacated = revision.vacated.to_vec();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let current = current_generation(&*transaction)?;
            if current != expected {
                return Ok(RegistryGraphWrite::Conflict { current });
            }
            if let Some(occupied) = occupied_namespace(&*transaction, &vacated)? {
                return Ok(RegistryGraphWrite::HostedNotEmpty(occupied));
            }
            let generation = current.map_or(1, |current| current + 1);
            transaction.execute(
                "INSERT INTO registry_graph_revisions
                 (generation, document, actor_id, created_at, restored_from)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![to_column(generation), document, actor_id, now_millis(), restored_from],
            )?;
            let oldest_kept = generation.saturating_sub(REGISTRY_GRAPH_REVISIONS_KEPT - 1);
            transaction.execute(
                "DELETE FROM registry_graph_revisions WHERE generation < ?1",
                params![to_column(oldest_kept)],
            )?;
            cdn::enqueue(&*transaction, "*")?;
            transaction.commit()?;
            Ok(RegistryGraphWrite::Written(generation))
        })
        .await
    }

    /// Whether the hosted `namespace` holds any package of `ecosystem`.
    pub async fn hosted_namespace_has_packages(
        &self,
        namespace: &str,
        ecosystem: Ecosystem,
    ) -> Result<bool> {
        let namespace = namespace.to_string();
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            Ok(connection
                .query_optional(
                    "SELECT 1 FROM packages WHERE namespace = ?1 AND ecosystem = ?2 LIMIT 1",
                    params![namespace, ecosystem.as_str()],
                )?
                .is_some())
        })
        .await
    }
}

/// The first of `namespaces` that holds a package.
fn occupied_namespace(
    connection: &dyn Connection,
    namespaces: &[HostedNamespace],
) -> Result<Option<HostedNamespace>> {
    for hosted in namespaces {
        connection.execute(LOCK_NAMESPACE, params![hosted.namespace])?;
        let found = connection.query_optional(
            NAMESPACE_HAS_PACKAGES.get(connection.dialect()),
            params![hosted.namespace, hosted.ecosystem.as_str()],
        )?;
        if found.is_some() {
            return Ok(Some(hosted.clone()));
        }
    }
    Ok(None)
}

fn current_generation(connection: &dyn Connection) -> Result<Option<u64>> {
    connection
        .query_optional(
            "SELECT generation FROM registry_graph_revisions ORDER BY generation DESC LIMIT 1",
            params![],
        )?
        .map(|mut row| decode_generation(row.get(0)?))
        .transpose()
}

fn stored_graph(mut row: Row) -> Result<StoredRegistryGraph> {
    Ok(StoredRegistryGraph {
        generation: decode_generation(row.get(0)?)?,
        document: row.get(1)?,
        actor_id: row.get(2)?,
        created_at: row.get(3)?,
        restored_from: row.get::<Option<i64>>(4)?.map(to_generation),
    })
}

fn revision_summary(mut row: Row) -> Result<RegistryGraphRevision> {
    Ok(RegistryGraphRevision {
        generation: decode_generation(row.get(0)?)?,
        actor_id: row.get(1)?,
        created_at: row.get(2)?,
        restored_from: row.get::<Option<i64>>(3)?.map(to_generation),
    })
}

fn decode_generation(value: i64) -> Result<u64> {
    if value < 1 {
        return Err(RegistryError::Internal {
            reason: format!("the stored registry graph has an invalid generation {value}"),
        });
    }
    Ok(to_generation(value))
}
