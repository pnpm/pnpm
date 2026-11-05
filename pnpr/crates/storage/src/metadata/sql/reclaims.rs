//! The rows that fence the removal of unreferenced archives (see
//! [`crate::metadata::ArchiveFence`]).
//!
//! Each step is one write transaction, so a remote database answers it in
//! two round trips: one that takes the write lock and reads what the step
//! compares with, and one that writes and commits.

use super::{
    PackageKey, SqlMetadata,
    adapter::{Connection, Query, Row},
    advance_catalog,
    batch::WriteTransaction,
    commit::package_row_query,
    to_column, to_generation,
};
use crate::metadata::{ArchiveFence, ArchiveRef, ReclaimTarget};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::sync::Arc;

/// How long, in seconds, the row of a finished reclaim is kept: far longer
/// than a publish can take between reading its fence token and committing.
const DONE_RETENTION_SECS: i64 = 24 * 60 * 60;

/// Drops the rows of reclaims finished more than [`DONE_RETENTION_SECS`]
/// ago.
const PRUNE_DONE: &str = "DELETE FROM archive_reclaims
     WHERE catalog = ?1 AND done_at IS NOT NULL
       AND done_at < CAST(strftime('%s', 'now') AS INTEGER) - ?2";

/// The archives of a fence that are marked, or were reclaimed after the
/// fence's token. `?3` is a JSON array of `[ecosystem, name, artifact]`.
const FENCE_QUERY: &str = "SELECT ecosystem, name, artifact FROM archive_reclaims
     WHERE catalog = ?1
       AND (done_generation IS NULL OR done_generation > ?2)
       AND (ecosystem, name, artifact) IN (
           SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]'),
                  json_extract(value, '$[2]')
           FROM json_each(?3))
     LIMIT 1";

/// The read a commit adds to its first round trip to check its fence.
pub(super) struct FenceRead {
    token: i64,
    archives: String,
}

impl FenceRead {
    /// `None` for a fence with no archives, which needs no read.
    pub(super) fn new(fence: &ArchiveFence) -> Option<Self> {
        if fence.archives.is_empty() {
            return None;
        }
        let archives: Vec<[&str; 3]> = fence.archives
            .iter()
            .map(|archive| [archive.ecosystem.as_str(), &archive.name, &archive.artifact])
            .collect();
        let archives = serde_json::to_string(&archives).expect("strings serialize");
        Some(Self { token: to_column(fence.token), archives })
    }

    pub(super) fn query<'read>(&'read self, catalog: &'read str) -> Query<'read> {
        query!(FENCE_QUERY, catalog, &self.token, self.archives.as_str())
    }

    /// Refuse the commit when the read found an archive it depends on.
    pub(super) fn check(rows: Vec<Row>) -> Result<()> {
        let Some(mut row) = rows.into_iter().next() else { return Ok(()) };
        let name: String = row.get(1)?;
        let artifact: String = row.get(2)?;
        Err(RegistryError::ArchiveReclaimed { package: name, artifact })
    }
}

impl SqlMetadata {
    /// Mark the archives of each target whose package is still at its
    /// expected generation, or still without metadata, as being reclaimed,
    /// and return them. From now on no commit that depends on one of them
    /// commits. Rows of reclaims finished a day ago go in the same
    /// transaction.
    pub async fn mark_reclaims(
        self: &Arc<Self>,
        catalog: &str,
        targets: Vec<ReclaimTarget>,
    ) -> Result<Vec<ArchiveRef>> {
        let catalog = catalog.to_string();
        self.write(move |connection| mark(connection, &catalog, &targets)).await
    }

    /// Record that the marked `archives` are gone, under a new generation of
    /// `catalog`: a commit that read an earlier generation before it
    /// promoted one of them is refused, a later one is not. Rows of reclaims
    /// finished a day ago go in the same transaction.
    pub async fn finish_reclaims(
        self: &Arc<Self>,
        catalog: &str,
        archives: Vec<ArchiveRef>,
    ) -> Result<()> {
        if archives.is_empty() {
            return Ok(());
        }
        let catalog = catalog.to_string();
        self.write(move |connection| finish(connection, &catalog, &archives)).await
    }

    /// The archives of `catalog` marked but not yet recorded gone: a reclaim
    /// that stopped between its steps.
    pub async fn pending_reclaims(self: &Arc<Self>, catalog: &str) -> Result<Vec<ArchiveRef>> {
        let catalog = catalog.to_string();
        self.read(catalog.clone(), super::Freshness::Current, move |connection, _| {
            let rows = connection.query(
                "SELECT ecosystem, name, artifact FROM archive_reclaims
                 WHERE catalog = ?1 AND done_generation IS NULL
                 ORDER BY ecosystem, name, artifact",
                params![catalog],
            )?;
            rows.into_iter()
                .map(archive_from_row)
                .collect()
        })
        .await
    }
}

fn archive_from_row(mut row: Row) -> Result<ArchiveRef> {
    let text: String = row.get(0)?;
    let ecosystem = Ecosystem::all()
        .find(|known| known.as_str() == text)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("an archive reclaim names the unknown ecosystem {text:?}"),
        })?;
    Ok(ArchiveRef { ecosystem, name: row.get(1)?, artifact: row.get(2)? })
}

fn mark(
    connection: &dyn Connection,
    catalog: &str,
    targets: &[ReclaimTarget],
) -> Result<Vec<ArchiveRef>> {
    let transaction = WriteTransaction::begin(connection)?;
    let keys: Vec<PackageKey> = targets
        .iter()
        .map(|target| PackageKey {
            catalog: catalog.to_string(),
            ecosystem: target.ecosystem,
            name: target.name.clone(),
        })
        .collect();
    let reads: Vec<Query<'_>> = keys
        .iter()
        .map(package_row_query)
        .collect();
    let results = transaction.query_all(&reads)?;
    let mut marked = Vec::new();
    for (target, rows) in targets.iter().zip(results) {
        let current = rows
            .into_iter()
            .next()
            .map(|mut row| row.get(0).map(to_generation))
            .transpose()?;
        if current != target.expected {
            continue;
        }
        for artifact in &target.artifacts {
            transaction.execute(
                "INSERT INTO archive_reclaims (catalog, ecosystem, name, artifact)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (catalog, ecosystem, name, artifact)
                 DO UPDATE SET done_generation = NULL, done_at = NULL",
                params![catalog, target.ecosystem.as_str(), target.name, artifact],
            )?;
            marked.push(ArchiveRef {
                ecosystem: target.ecosystem,
                name: target.name.clone(),
                artifact: artifact.clone(),
            });
        }
    }
    transaction.execute(PRUNE_DONE, params![catalog, DONE_RETENTION_SECS])?;
    transaction.commit()?;
    Ok(marked)
}

fn finish(connection: &dyn Connection, catalog: &str, archives: &[ArchiveRef]) -> Result<()> {
    let transaction = WriteTransaction::begin(connection)?;
    let current = transaction
        .query_all(&[Query::new(
            "SELECT generation FROM catalogs WHERE catalog = ?1",
            params![catalog],
        )])?
        .into_iter()
        .next()
        .and_then(|rows| rows.into_iter().next())
        .map(|mut row| row.get(0).map(to_generation))
        .transpose()?
        .unwrap_or(0);
    let generation = current + 1;
    for archive in archives {
        transaction.execute(
            "UPDATE archive_reclaims
             SET done_generation = ?5, done_at = CAST(strftime('%s', 'now') AS INTEGER)
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND artifact = ?4
               AND done_generation IS NULL",
            params![
                catalog,
                archive.ecosystem.as_str(),
                archive.name,
                archive.artifact,
                to_column(generation),
            ],
        )?;
    }
    advance_catalog(&*transaction, catalog, generation)?;
    transaction.execute(PRUNE_DONE, params![catalog, DONE_RETENTION_SECS])?;
    transaction.commit()
}
