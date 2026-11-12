//! The transitions that reclaim a blob (see [`crate::metadata::BlobRow`]).
//!
//! Each transition that changes a row is one write transaction, so a remote
//! database answers it in two round trips: one that takes the write lock and
//! reads what the transition compares with, and one that writes and commits.

use super::{
    SqlMetadata,
    adapter::{Connection, Query, Row},
    batch::WriteTransaction,
    blobs::{ROWS, id_from, json_array, rows_from},
    to_column, to_generation,
    transactions::Freshness,
};
use crate::{
    BlobId,
    metadata::{BlobRef, BlobRow, BlobState, Uncommitted},
};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use pnpr_sql::DialectSql;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// The live blobs of a namespace nothing maps, with their rows. `?3` narrows
/// them to a JSON array of identities, or is `NULL` for all of them.
const UNMAPPED: DialectSql = DialectSql {
    sqlite: "SELECT b.blob_id, b.incarnation, b.state, b.size FROM blobs b
             WHERE b.namespace = ?1 AND b.ecosystem = ?2 AND b.state = 'live'
               AND (?3 IS NULL OR b.blob_id IN (SELECT value FROM json_each(?3)))
               AND NOT EXISTS (
                   SELECT 1 FROM artifact_blobs a
                   WHERE a.namespace = b.namespace AND a.ecosystem = b.ecosystem
                     AND a.blob_id = b.blob_id)",
    postgres: "SELECT b.blob_id, b.incarnation, b.state, b.size FROM blobs b
               WHERE b.namespace = ?1 AND b.ecosystem = ?2 AND b.state = 'live'
                 AND (?3 IS NULL OR b.blob_id IN (SELECT value FROM json_array_elements_text(CAST(?3 AS JSON)) AS elements(value)))
                 AND NOT EXISTS (
                     SELECT 1 FROM artifact_blobs a
                     WHERE a.namespace = b.namespace AND a.ecosystem = b.ecosystem
                       AND a.blob_id = b.blob_id)",
    mysql: "SELECT b.blob_id, b.incarnation, b.state, b.size FROM blobs b
            WHERE b.namespace = ?1 AND b.ecosystem = ?2 AND b.state = 'live'
              AND (?3 IS NULL OR b.blob_id IN (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?3, '$[*]' COLUMNS (value JSON PATH '$')) AS elements))
              AND NOT EXISTS (
                  SELECT 1 FROM artifact_blobs a
                  WHERE a.namespace = b.namespace AND a.ecosystem = b.ecosystem
                    AND a.blob_id = b.blob_id)",
};

impl SqlMetadata {
    /// The rows of `ids` in `namespace`'s `ecosystem`, read from the primary.
    pub async fn blob_rows(
        self: &Arc<Self>,
        namespace: &str,
        ecosystem: Ecosystem,
        ids: Vec<BlobId>,
    ) -> Result<BTreeMap<BlobId, BlobRow>> {
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let namespace = namespace.to_string();
        self.read(namespace.clone(), Freshness::Current, move |connection, _| {
            let ids: Vec<&str> = ids.iter().map(BlobId::as_str).collect();
            let ids = json_array(&ids);
            let sql = ROWS.get(connection.dialect());
            rows_from(connection.query(sql, params![namespace, ecosystem.as_str(), ids])?)
        })
        .await
    }

    /// The live blobs of `namespace`'s `ecosystem` that nothing maps, read
    /// from the primary: what a reclaim of the namespace takes.
    pub async fn unmapped_blobs(
        self: &Arc<Self>,
        namespace: &str,
        ecosystem: Ecosystem,
    ) -> Result<BTreeMap<BlobId, BlobRow>> {
        let namespace = namespace.to_string();
        self.read(namespace.clone(), Freshness::Current, move |connection, _| {
            let all: Option<String> = None;
            rows_from(connection.query(
                UNMAPPED.get(connection.dialect()),
                params![namespace, ecosystem.as_str(), all],
            )?)
        })
        .await
    }

    /// Move the blobs among `ids` that are live and that nothing maps to
    /// `reclaiming`, and return their incarnations. From now on no commit
    /// depends on them, and their keys may be removed at any time.
    pub async fn mark_reclaiming(
        self: &Arc<Self>,
        namespace: &str,
        ecosystem: Ecosystem,
        ids: Vec<BlobId>,
    ) -> Result<Vec<BlobRef>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let namespace = namespace.to_string();
        self.write(move |connection| mark(connection, (&namespace, ecosystem), &ids)).await
    }

    /// Record that the keys of `reclaimed`, each `reclaiming`, are removed.
    pub async fn finish_reclaims(
        self: &Arc<Self>,
        namespace: &str,
        ecosystem: Ecosystem,
        reclaimed: Vec<BlobRef>,
    ) -> Result<()> {
        if reclaimed.is_empty() {
            return Ok(());
        }
        let namespace = namespace.to_string();
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let now = crate::publish::now_millis();
            let sql = "UPDATE blobs SET state = 'gone', changed_at = ?5
                 WHERE namespace = ?1 AND ecosystem = ?2 AND blob_id = ?3 AND incarnation = ?4
                   AND state = 'reclaiming'";
            let ecosystem = ecosystem.as_str();
            for blob in &reclaimed {
                let (id, incarnation) = (blob.id.as_str(), to_column(blob.incarnation));
                transaction.execute(sql, params![namespace, ecosystem, id, incarnation, now])?;
            }
            transaction.commit()
        })
        .await
    }

    /// The incarnations of `namespace`'s `ecosystem` that have been
    /// `reclaiming` for at least `lease`: reclaims that stopped before they
    /// finished, read from the primary.
    pub async fn stale_reclaims(
        self: &Arc<Self>,
        namespace: &str,
        ecosystem: Ecosystem,
        lease: Duration,
    ) -> Result<BTreeMap<BlobRef, u64>> {
        let namespace = namespace.to_string();
        let lease = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX);
        self.read(namespace.clone(), Freshness::Current, move |connection, _| {
            let now = crate::publish::now_millis();
            let sql = "SELECT blob_id, incarnation, size FROM blobs
                 WHERE namespace = ?1 AND ecosystem = ?2 AND state = 'reclaiming'
                   AND changed_at <= ?4 - ?3";
            connection
                .query(sql, params![namespace, ecosystem.as_str(), lease, now])?
                .into_iter()
                .map(|mut row| {
                    let size = to_generation(row.get(2)?);
                    Ok((blob_ref_from(row)?, size))
                })
                .collect()
        })
        .await
    }

    /// Settle the incarnations stored in `namespace`'s `ecosystem` that no
    /// commit made the live one, and return those whose keys may be removed:
    /// one below its blob's incarnation, or at it once gone, can never be
    /// committed. One above it, or of a blob without a row, may be a publish
    /// on its way: when `fence` holds for it, it is recorded `gone`, which
    /// refuses that publish's commit, and returned too. One above a live
    /// incarnation is left: no commit can make it live while that one is,
    /// and it is settled once that one is reclaimed.
    pub async fn settle_stored(
        self: &Arc<Self>,
        namespace: &str,
        ecosystem: Ecosystem,
        stored: Vec<Uncommitted>,
    ) -> Result<Vec<BlobRef>> {
        if stored.is_empty() {
            return Ok(Vec::new());
        }
        let namespace = namespace.to_string();
        self.write(move |connection| settle(connection, (&namespace, ecosystem), &stored)).await
    }
}

fn blob_ref_from(mut row: Row) -> Result<BlobRef> {
    Ok(BlobRef { id: id_from(&row.get::<String>(0)?)?, incarnation: to_generation(row.get(1)?) })
}

fn mark(
    connection: &dyn Connection,
    (namespace, ecosystem): (&str, Ecosystem),
    ids: &[BlobId],
) -> Result<Vec<BlobRef>> {
    let transaction = WriteTransaction::begin(connection)?;
    let ids: Vec<&str> = ids.iter().map(BlobId::as_str).collect();
    let ids = Some(json_array(&ids));
    let ecosystem_name = ecosystem.as_str();
    let read =
        Query::new(UNMAPPED.get(connection.dialect()), params![namespace, ecosystem_name, ids]);
    let rows = rows_from(
        transaction
            .query_all(&[read])?
            .into_iter()
            .flatten()
            .collect(),
    )?;
    let now = crate::publish::now_millis();
    let sql = "UPDATE blobs SET state = 'reclaiming', changed_at = ?5
         WHERE namespace = ?1 AND ecosystem = ?2 AND blob_id = ?3 AND incarnation = ?4";
    let mut marked = Vec::with_capacity(rows.len());
    for (id, row) in rows {
        let incarnation = to_column(row.incarnation);
        transaction.execute(
            sql,
            params![namespace, ecosystem.as_str(), id.as_str(), incarnation, now],
        )?;
        marked.push(BlobRef { id, incarnation: row.incarnation });
    }
    transaction.commit()?;
    Ok(marked)
}

/// What [`SqlMetadata::settle_stored`] does with one stored incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settled {
    Keep,
    Remove,
    Fence,
}

fn settled(row: Option<&BlobRow>, incarnation: u64, fence: bool) -> Settled {
    let fenced = if fence { Settled::Fence } else { Settled::Keep };
    let Some(row) = row else { return fenced };
    match incarnation.cmp(&row.incarnation) {
        std::cmp::Ordering::Less => Settled::Remove,
        std::cmp::Ordering::Equal if row.state == BlobState::Gone => Settled::Remove,
        std::cmp::Ordering::Equal => Settled::Keep,
        std::cmp::Ordering::Greater if row.state == BlobState::Live => Settled::Keep,
        std::cmp::Ordering::Greater => fenced,
    }
}

fn settle(
    connection: &dyn Connection,
    namespace: (&str, Ecosystem),
    stored: &[Uncommitted],
) -> Result<Vec<BlobRef>> {
    let transaction = WriteTransaction::begin(connection)?;
    let mut rows =
        read_rows(&*transaction, namespace, stored.iter().map(|stored| &stored.blob.id))?;
    let mut ordered: Vec<&Uncommitted> = stored.iter().collect();
    ordered.sort_by(|left, right| left.blob.cmp(&right.blob));
    let mut removable = Vec::new();
    for Uncommitted { blob, may_fence } in ordered {
        match settled(rows.get(&blob.id), blob.incarnation, *may_fence) {
            Settled::Keep => continue,
            Settled::Remove => {}
            Settled::Fence => {
                write_gone(&*transaction, namespace, blob)?;
                let size = rows.get(&blob.id).map_or(0, |row| row.size);
                let gone = BlobRow { incarnation: blob.incarnation, state: BlobState::Gone, size };
                rows.insert(blob.id.clone(), gone);
            }
        }
        removable.push(blob.clone());
    }
    transaction.commit()?;
    Ok(removable)
}

/// The rows of `ids` in `namespace`'s `ecosystem`, read in one round trip.
fn read_rows<'id>(
    connection: &dyn Connection,
    (namespace, ecosystem): (&str, Ecosystem),
    ids: impl Iterator<Item = &'id BlobId>,
) -> Result<BTreeMap<BlobId, BlobRow>> {
    let ids: Vec<&str> = ids.map(BlobId::as_str).collect();
    let ids = json_array(&ids);
    let ecosystem = ecosystem.as_str();
    let read = Query::new(ROWS.get(connection.dialect()), params![namespace, ecosystem, ids]);
    rows_from(
        connection
            .query_all(&[read])?
            .into_iter()
            .flatten()
            .collect(),
    )
}

const WRITE_GONE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO blobs (namespace, ecosystem, blob_id, incarnation, state, size, changed_at)
     VALUES (?1, ?2, ?3, ?4, 'gone', 0, ?5)
     ON CONFLICT (namespace, ecosystem, blob_id) DO UPDATE
     SET incarnation = excluded.incarnation, state = 'gone',
         changed_at = excluded.changed_at",
    "INSERT INTO blobs (namespace, ecosystem, blob_id, incarnation, state, size, changed_at)
     VALUES (?1, ?2, ?3, ?4, 'gone', 0, ?5)
     AS excluded ON DUPLICATE KEY UPDATE incarnation = excluded.incarnation, state = 'gone',
         changed_at = excluded.changed_at",
);

/// Record `blob`'s incarnation as the blob's current one, gone.
fn write_gone(
    transaction: &dyn Connection,
    (namespace, ecosystem): (&str, Ecosystem),
    blob: &BlobRef,
) -> Result<()> {
    let now = crate::publish::now_millis();
    let incarnation = to_column(blob.incarnation);
    transaction.execute(
        WRITE_GONE.get(transaction.dialect()),
        params![namespace, ecosystem.as_str(), blob.id.as_str(), incarnation, now],
    )
}
