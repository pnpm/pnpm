//! The rows that fence the removal of unreferenced archives (see
//! [`crate::metadata::ArchiveFence`]).
//!
//! Each step is one write transaction, so a remote database answers it in
//! two round trips: one that takes the write lock and reads what the step
//! compares with, and one that writes and commits.
//!
//! A marked archive belongs to the reclaim that marked it, named by an owner
//! token. Another reclaim marks it only once it is finished, and takes it
//! over only once its mark is older than the lease the caller allows, so two
//! reclaims never remove one archive at once. Before it removes anything a
//! reclaim checks that its marks are still its own, and it records only its
//! own marks as finished.

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
use std::{collections::BTreeSet, sync::Arc, time::Duration};

/// How long, in seconds, the row of a finished reclaim is kept. Dropping it
/// raises the catalog's reclaim horizon to its generation, so a commit that
/// read an older generation is refused however long it took.
const DONE_RETENTION_SECS: i64 = 24 * 60 * 60;

/// The current time in seconds since the Unix epoch, as the database tells
/// it, so every instance reads one clock.
const NOW: &str = "CAST(strftime('%s', 'now') AS INTEGER)";

/// Raises the catalog's reclaim horizon to the newest generation among the
/// finished rows about to be dropped, once [`ENSURE_HORIZON`] has given the
/// catalog one. `?2` is [`DONE_RETENTION_SECS`].
const RAISE_HORIZON: &str = "UPDATE reclaim_horizons
     SET generation = MAX(generation, COALESCE((
         SELECT MAX(done_generation) FROM archive_reclaims
         WHERE catalog = ?1 AND done_at IS NOT NULL
           AND done_at < CAST(strftime('%s', 'now') AS INTEGER) - ?2), 0))
     WHERE catalog = ?1";

/// Gives the catalog a reclaim horizon, `0` until a row is dropped.
const ENSURE_HORIZON: &str =
    "INSERT INTO reclaim_horizons (catalog, generation) VALUES (?1, 0) ON CONFLICT DO NOTHING";

/// Drops the rows of reclaims finished more than [`DONE_RETENTION_SECS`]
/// ago, once [`RAISE_HORIZON`] has recorded them.
const PRUNE_DONE: &str = "DELETE FROM archive_reclaims
     WHERE catalog = ?1 AND done_at IS NOT NULL
       AND done_at < CAST(strftime('%s', 'now') AS INTEGER) - ?2";

/// The archives of a fence that are marked, or were reclaimed after the
/// fence's token, and a row when the catalog's reclaim horizon passed the
/// token. `?3` is a JSON array of `[ecosystem, name, artifact]`.
const FENCE_QUERY: &str = "SELECT name, artifact FROM archive_reclaims
     WHERE catalog = ?1
       AND (done_generation IS NULL OR done_generation > ?2)
       AND (ecosystem, name, artifact) IN (
           SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]'),
                  json_extract(value, '$[2]')
           FROM json_each(?3))
     UNION ALL
     SELECT NULL, NULL FROM reclaim_horizons WHERE catalog = ?1 AND generation > ?2
     LIMIT 1";

/// The read a commit adds to its first round trip to check its fence.
pub(super) struct FenceRead {
    token: i64,
    archives: String,
    /// The fence's first archive, which a refusal by the horizon names.
    first: ArchiveRef,
}

impl FenceRead {
    /// `None` for a fence with no archives, which needs no read.
    pub(super) fn new(fence: &ArchiveFence) -> Option<Self> {
        let first = fence.archives.first()?.clone();
        let archives: Vec<[&str; 3]> = fence.archives
            .iter()
            .map(|archive| [archive.ecosystem.as_str(), &archive.name, &archive.artifact])
            .collect();
        let archives = serde_json::to_string(&archives).expect("strings serialize");
        Some(Self { token: to_column(fence.token), archives, first })
    }

    pub(super) fn query<'read>(&'read self, catalog: &'read str) -> Query<'read> {
        query!(FENCE_QUERY, catalog, &self.token, self.archives.as_str())
    }

    /// Refuse the commit when the read found an archive it depends on, or
    /// that the fence's token is older than what the catalog remembers.
    pub(super) fn check(&self, rows: Vec<Row>) -> Result<()> {
        let Some(mut row) = rows.into_iter().next() else { return Ok(()) };
        let name: Option<String> = row.get(0)?;
        let artifact: Option<String> = row.get(1)?;
        Err(RegistryError::ArchiveReclaimed {
            package: name.unwrap_or_else(|| self.first.name.clone()),
            artifact: artifact.unwrap_or_else(|| self.first.artifact.clone()),
        })
    }
}

/// A token naming one reclaim, the owner of the archives it marks.
#[must_use]
pub fn new_reclaim_owner() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    bytes
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

impl SqlMetadata {
    /// Mark, for `owner`, the archives of each target whose package is still
    /// at its expected generation, or still without metadata, and return
    /// them. An archive another reclaim has marked and not finished is left
    /// to it. From now on no commit that depends on a marked archive
    /// commits.
    pub async fn mark_reclaims(
        self: &Arc<Self>,
        catalog: &str,
        owner: &str,
        targets: Vec<ReclaimTarget>,
    ) -> Result<Vec<ArchiveRef>> {
        let (catalog, owner) = (catalog.to_string(), owner.to_string());
        self.write(move |connection| mark(connection, &catalog, &owner, &targets)).await
    }

    /// Take over, for `owner`, the pending marks of `catalog` older than
    /// `lease`: a reclaim that stopped before it finished. `filter` keeps
    /// the marks to take.
    pub async fn claim_stale_reclaims(
        self: &Arc<Self>,
        catalog: &str,
        owner: &str,
        lease: Duration,
        filter: impl Fn(&ArchiveRef) -> bool + Send + 'static,
    ) -> Result<Vec<ArchiveRef>> {
        let (catalog, owner) = (catalog.to_string(), owner.to_string());
        let lease = i64::try_from(lease.as_secs()).unwrap_or(i64::MAX);
        self.write(move |connection| claim_stale(connection, (&catalog, &owner), lease, &filter))
            .await
    }

    /// The archives among `archives` still marked by `owner`, read from the
    /// primary. A reclaim checks this right before it removes them.
    pub async fn owned_reclaims(
        self: &Arc<Self>,
        catalog: &str,
        owner: &str,
        archives: Vec<ArchiveRef>,
    ) -> Result<Vec<ArchiveRef>> {
        let (catalog, owner) = (catalog.to_string(), owner.to_string());
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let held = marks_of(&*transaction, &catalog, Some(&owner))?;
            transaction.commit()?;
            Ok(archives
                .iter()
                .filter(|archive| held.contains(archive))
                .cloned()
                .collect())
        })
        .await
    }

    /// Record that `owner`'s marked `archives` are gone, under a new
    /// generation of `catalog`: a commit that read an earlier generation
    /// before it promoted one of them is refused, a later one is not. Rows
    /// of reclaims finished a day ago go in the same transaction, and raise
    /// the catalog's reclaim horizon.
    pub async fn finish_reclaims(
        self: &Arc<Self>,
        catalog: &str,
        owner: &str,
        archives: Vec<ArchiveRef>,
    ) -> Result<()> {
        if archives.is_empty() {
            return Ok(());
        }
        let (catalog, owner) = (catalog.to_string(), owner.to_string());
        self.write(move |connection| finish(connection, (&catalog, &owner), &archives)).await
    }

    /// The archives of `catalog` marked but not yet recorded gone.
    pub async fn pending_reclaims(self: &Arc<Self>, catalog: &str) -> Result<Vec<ArchiveRef>> {
        let catalog = catalog.to_string();
        self.read(catalog.clone(), super::Freshness::Current, move |connection, _| {
            Ok(marks_of(connection, &catalog, None)?.into_iter().collect())
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

/// The pending marks of `catalog`, of `owner` when one is given.
fn marks_of(
    connection: &dyn Connection,
    catalog: &str,
    owner: Option<&str>,
) -> Result<BTreeSet<ArchiveRef>> {
    let rows = connection.query(
        "SELECT ecosystem, name, artifact FROM archive_reclaims
         WHERE catalog = ?1 AND done_generation IS NULL AND (?2 IS NULL OR owner = ?2)",
        params![catalog, owner],
    )?;
    rows.into_iter()
        .map(archive_from_row)
        .collect()
}

fn mark(
    connection: &dyn Connection,
    catalog: &str,
    owner: &str,
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
    let mut reads: Vec<Query<'_>> = keys
        .iter()
        .map(package_row_query)
        .collect();
    reads.push(query!(
        "SELECT ecosystem, name, artifact FROM archive_reclaims
         WHERE catalog = ?1 AND done_generation IS NULL",
        catalog,
    ));
    let mut results = transaction.query_all(&reads)?;
    let taken: BTreeSet<ArchiveRef> = results
        .pop()
        .unwrap_or_default()
        .into_iter()
        .map(archive_from_row)
        .collect::<Result<_>>()?;
    let mut marked = Vec::new();
    for (target, rows) in targets.iter().zip(results) {
        if generation_of(rows)? == target.expected {
            let free = target_archives(target).filter(|archive| !taken.contains(archive));
            for archive in free {
                write_mark(&*transaction, (catalog, owner), &archive)?;
                marked.push(archive);
            }
        }
    }
    prune(&*transaction, catalog)?;
    transaction.commit()?;
    Ok(marked)
}

/// The generation of the package row in `rows`, `None` without one.
fn generation_of(rows: Vec<Row>) -> Result<Option<u64>> {
    rows.into_iter()
        .next()
        .map(|mut row| row.get(0).map(to_generation))
        .transpose()
}

fn target_archives(target: &ReclaimTarget) -> impl Iterator<Item = ArchiveRef> + '_ {
    target.artifacts
        .iter()
        .map(|artifact| ArchiveRef {
            ecosystem: target.ecosystem,
            name: target.name.clone(),
            artifact: artifact.clone(),
        })
}

/// Mark `archive` for `owner`, pending again if it was reclaimed before.
fn write_mark(
    transaction: &dyn Connection,
    (catalog, owner): (&str, &str),
    archive: &ArchiveRef,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO archive_reclaims (catalog, ecosystem, name, artifact, owner, marked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, {NOW})
         ON CONFLICT (catalog, ecosystem, name, artifact)
         DO UPDATE SET owner = excluded.owner, marked_at = excluded.marked_at,
                       done_generation = NULL, done_at = NULL",
    );
    transaction.execute(
        &sql,
        params![catalog, archive.ecosystem.as_str(), archive.name, archive.artifact, owner],
    )
}

fn claim_stale(
    connection: &dyn Connection,
    (catalog, owner): (&str, &str),
    lease: i64,
    filter: &dyn Fn(&ArchiveRef) -> bool,
) -> Result<Vec<ArchiveRef>> {
    let transaction = WriteTransaction::begin(connection)?;
    let sql = format!(
        "SELECT ecosystem, name, artifact FROM archive_reclaims
         WHERE catalog = ?1 AND done_generation IS NULL AND marked_at <= {NOW} - ?2",
    );
    let stale = transaction.query_all(&[Query::new(&sql, params![catalog, lease])])?;
    let claimed: Vec<ArchiveRef> = stale
        .into_iter()
        .flatten()
        .map(archive_from_row)
        .filter(|archive| archive.as_ref().map_or(true, filter))
        .collect::<Result<_>>()?;
    for archive in &claimed {
        write_mark(&*transaction, (catalog, owner), archive)?;
    }
    transaction.commit()?;
    Ok(claimed)
}

fn finish(
    connection: &dyn Connection,
    (catalog, owner): (&str, &str),
    archives: &[ArchiveRef],
) -> Result<()> {
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
    let sql = format!(
        "UPDATE archive_reclaims SET done_generation = ?6, done_at = {NOW}, owner = NULL
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND artifact = ?4
           AND owner = ?5 AND done_generation IS NULL",
    );
    for archive in archives {
        transaction.execute(
            &sql,
            params![
                catalog,
                archive.ecosystem.as_str(),
                archive.name,
                archive.artifact,
                owner,
                to_column(generation),
            ],
        )?;
    }
    advance_catalog(&*transaction, catalog, generation)?;
    prune(&*transaction, catalog)?;
    transaction.commit()
}

/// Drop the rows of reclaims finished a day ago, raising the catalog's
/// reclaim horizon to them first.
fn prune(transaction: &dyn Connection, catalog: &str) -> Result<()> {
    transaction.execute(ENSURE_HORIZON, params![catalog])?;
    transaction.execute(RAISE_HORIZON, params![catalog, DONE_RETENTION_SECS])?;
    transaction.execute(PRUNE_DONE, params![catalog, DONE_RETENTION_SECS])
}
