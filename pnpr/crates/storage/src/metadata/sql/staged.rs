//! The records of staged archives (see [`crate::metadata::StagedRecord`]).
//!
//! Every change is one write transaction that reads the record and writes
//! what the caller computes from it, under the write lock, so a remote
//! database answers it in two round trips.

use super::{
    SqlMetadata,
    adapter::{Connection, Query, Row},
    batch::WriteTransaction,
    to_column, to_generation,
    transactions::Freshness,
};
use crate::metadata::{StagedRecord, StagedState};
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

/// A principal's staged archive, by the hex digest of its bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StagedId {
    pub(crate) principal: String,
    pub(crate) digest: String,
}

/// A staged archive's record, with the archive it records.
pub(crate) type StagedEntry = (StagedId, StagedRecord);

/// The record a change found stored, and the one it wrote.
pub(crate) type StagedChange = (Option<StagedRecord>, Option<StagedRecord>);

impl SqlMetadata {
    /// The record of `id`, read from the primary.
    pub(crate) async fn staged_record(
        self: &Arc<Self>,
        id: StagedId,
    ) -> Result<Option<StagedRecord>> {
        self.read(String::new(), Freshness::Current, move |connection, _| read_one(connection, &id))
            .await
    }

    /// Every record of `principal`, or of every principal for `None`, that
    /// no sweep took, read from the primary. A taken record only keeps its
    /// incarnation from being reused, which no caller of this needs.
    pub(crate) async fn staged_records(
        self: &Arc<Self>,
        principal: Option<String>,
    ) -> Result<Vec<StagedEntry>> {
        self.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query(
                    "SELECT principal, digest, incarnation, staged_at, size, state
                     FROM staged_archives
                     WHERE (?1 IS NULL OR principal = ?1) AND state != 'gone'",
                    params![principal],
                )?
                .into_iter()
                .map(entry_from)
                .collect()
        })
        .await
    }

    /// Replace the record of `id` with what `change` makes of the stored
    /// one, `None` when there is none, in one transaction. Returns the
    /// stored record and the one written, `None` when `change` kept the
    /// stored one.
    pub(crate) async fn change_staged_record(
        self: &Arc<Self>,
        id: StagedId,
        change: impl Fn(Option<&StagedRecord>) -> Option<StagedRecord> + Send + 'static,
    ) -> Result<StagedChange> {
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let stored = read_one(&*transaction, &id)?;
            let Some(record) = change(stored.as_ref()) else { return Ok((stored, None)) };
            write(&*transaction, &id, &record)?;
            transaction.commit()?;
            Ok((stored, Some(record)))
        })
        .await
    }
}

fn read_one(connection: &dyn Connection, id: &StagedId) -> Result<Option<StagedRecord>> {
    let read = Query::new(
        "SELECT principal, digest, incarnation, staged_at, size, state
         FROM staged_archives WHERE principal = ?1 AND digest = ?2",
        params![id.principal, id.digest],
    );
    connection
        .query_all(&[read])?
        .into_iter()
        .flatten()
        .next()
        .map(|row| entry_from(row).map(|(_, record)| record))
        .transpose()
}

fn write(transaction: &dyn Connection, id: &StagedId, record: &StagedRecord) -> Result<()> {
    transaction.execute(
        "INSERT INTO staged_archives (principal, digest, incarnation, staged_at, size, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (principal, digest) DO UPDATE
         SET incarnation = excluded.incarnation, staged_at = excluded.staged_at,
             size = excluded.size, state = excluded.state",
        params![
            id.principal,
            id.digest,
            to_column(record.incarnation),
            to_column(record.staged_at),
            to_column(record.size),
            record.state.as_str(),
        ],
    )
}

fn entry_from(mut row: Row) -> Result<StagedEntry> {
    let id = StagedId { principal: row.get(0)?, digest: row.get(1)? };
    let state: String = row.get(5)?;
    let state = StagedState::parse(&state)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("a staged archive is in the unknown state {state:?}"),
        })?;
    let record = StagedRecord {
        incarnation: to_generation(row.get(2)?),
        staged_at: to_generation(row.get(3)?),
        size: to_generation(row.get(4)?),
        state,
    };
    Ok((id, record))
}
