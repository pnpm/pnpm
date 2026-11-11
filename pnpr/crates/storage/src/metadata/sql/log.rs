//! The sequenced entries of each namespace's transparency logs, one log per
//! namespace and ecosystem.

use super::{PackageKey, adapter::Connection};
use crate::transparency_log::{ImportedLogEntry, LogId, RawLogState};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::ops::Range;

/// The entries of `log` at `indexes`, in order. Every index in the range
/// must have been sequenced.
pub(super) fn entries(
    connection: &dyn Connection,
    log: &LogId,
    indexes: Range<u64>,
) -> Result<Vec<Vec<u8>>> {
    let rows = connection.query(
        "SELECT entry FROM log_entries
         WHERE namespace = ?1 AND ecosystem = ?2 AND idx >= ?3 AND idx < ?4
         ORDER BY idx",
        params![
            log.namespace,
            log.ecosystem.as_str(),
            to_column(indexes.start),
            to_column(indexes.end)
        ],
    )?;
    let entries: Vec<Vec<u8>> = rows
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    if u64::try_from(entries.len()).ok() != Some(indexes.end.saturating_sub(indexes.start)) {
        return Err(RegistryError::Internal {
            reason: format!("the log {log} is missing entries in {indexes:?}"),
        });
    }
    Ok(entries)
}

/// The number of entries sequenced in `log`.
pub(super) fn size(connection: &dyn Connection, log: &LogId) -> Result<u64> {
    let mut row = connection.query_one(
        "SELECT COALESCE(MAX(idx) + 1, 0) FROM log_entries WHERE namespace = ?1 AND ecosystem = ?2",
        params![log.namespace, log.ecosystem.as_str()],
    )?;
    Ok(u64::try_from(row.get::<i64>(0)?).unwrap_or_default())
}

/// The tree `log` was integrated to: its size and root.
pub(super) fn head(connection: &dyn Connection, log: &LogId) -> Result<Option<(u64, Vec<u8>)>> {
    let row = connection.query_optional(
        "SELECT size, root FROM log_heads WHERE namespace = ?1 AND ecosystem = ?2",
        params![log.namespace, log.ecosystem.as_str()],
    )?;
    row.map(parse_head_row).transpose()
}

/// The integrated tree head and sequenced entry count of `log` in one round trip.
pub(super) fn head_and_size(connection: &dyn Connection, log: &LogId) -> Result<RawLogState> {
    let ecosystem = log.ecosystem.as_str();
    let reads = [
        query!(
            "SELECT size, root FROM log_heads WHERE catalog = ?1 AND ecosystem = ?2",
            &log.catalog,
            ecosystem,
        ),
        query!(
            "SELECT COALESCE(MAX(idx) + 1, 0) FROM log_entries WHERE catalog = ?1 AND ecosystem = ?2",
            &log.catalog,
            ecosystem,
        ),
    ];
    let [head_rows, size_rows] =
        <[Vec<super::adapter::Row>; 2]>::try_from(connection.query_all(&reads)?)
            .map_err(|_| RegistryError::Internal {
                reason: "a metadata batch did not answer every read of a log".to_string(),
            })?;
    let head = head_rows
        .into_iter()
        .next()
        .map(parse_head_row)
        .transpose()?;
    let sequenced = size_rows
        .into_iter()
        .next()
        .map(|mut row| row.get::<i64>(0))
        .transpose()?
        .and_then(|count| u64::try_from(count).ok())
        .unwrap_or_default();
    Ok(RawLogState { head, sequenced })
}

fn parse_head_row(mut row: super::adapter::Row) -> Result<(u64, Vec<u8>)> {
    Ok((u64::try_from(row.get::<i64>(0)?).unwrap_or_default(), row.get(1)?))
}

/// Record that `log` was integrated to the tree of `size` entries with
/// `root`, unless it already was to a tree at least that large.
pub(super) fn advance_head(
    connection: &dyn Connection,
    log: &LogId,
    size: u64,
    root: &[u8],
) -> Result<()> {
    let transaction = super::batch::WriteTransaction::begin(connection)?;
    if head(&*transaction, log)?.is_some_and(|(current, _)| current >= size) {
        return transaction.commit();
    }
    transaction.execute(
        "INSERT INTO log_heads (namespace, ecosystem, size, root) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (namespace, ecosystem) DO UPDATE SET size = excluded.size, root = excluded.root",
        params![log.namespace, log.ecosystem.as_str(), to_column(size), root],
    )?;
    transaction.commit()
}

/// Every log with a sequenced entry, with the number of its entries.
pub(super) fn sizes(connection: &dyn Connection) -> Result<Vec<(LogId, u64)>> {
    let rows = connection.query(
        "SELECT namespace, ecosystem, MAX(idx) + 1 FROM log_entries GROUP BY namespace, ecosystem",
        params![],
    )?;
    rows.into_iter()
        .map(|mut row| {
            let namespace: String = row.get(0)?;
            let ecosystem: String = row.get(1)?;
            let ecosystem = parse_ecosystem(&ecosystem)?;
            let size = u64::try_from(row.get::<i64>(2)?).unwrap_or_default();
            Ok((LogId { namespace, ecosystem }, size))
        })
        .collect()
}

/// Where the record `version` of `key` was accepted with is in its log: the
/// index of its entry and the record's digest.
pub(super) fn record_entry(
    connection: &dyn Connection,
    key: &PackageKey,
    version: &str,
) -> Result<Option<(u64, String)>> {
    let (namespace, ecosystem, name) = key.params();
    let row = connection.query_optional(
        "SELECT log_index, digest FROM release_records
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
        params![namespace, ecosystem, name, version],
    )?;
    row.map(|mut row| {
        let index = u64::try_from(row.get::<i64>(0)?).unwrap_or_default();
        Ok((index, row.get(1)?))
    })
    .transpose()
}

/// Append `imported`, starting at `start`, to `log`, verifying any prefix a
/// concurrent recovery already wrote and recording rolled-back npm versions in
/// `removed_versions` so their identities cannot be republished.
pub(super) fn import_entries(
    connection: &dyn Connection,
    log: &LogId,
    start: u64,
    imported: &[ImportedLogEntry],
) -> Result<()> {
    if imported.is_empty() {
        return Ok(());
    }
    let transaction = super::batch::WriteTransaction::begin(connection)?;
    let current = verify_import_overlap(&*transaction, log, start, imported)?;
    let end = start + u64::try_from(imported.len()).unwrap_or_default();
    if current < end {
        let skip = usize::try_from(current - start).unwrap_or(imported.len());
        insert_imported_entries(&*transaction, log, current, &imported[skip..])?;
        record_reimported_npm_versions(&*transaction, log, &imported[skip..])?;
    }
    transaction.commit()
}

fn verify_import_overlap(
    connection: &dyn Connection,
    log: &LogId,
    start: u64,
    imported: &[ImportedLogEntry],
) -> Result<u64> {
    let current = size(connection, log)?;
    if current < start {
        return Err(RegistryError::Internal {
            reason: format!(
                "the log {log} shrank from {start} to {current} entries during recovery",
            ),
        });
    }
    let end = start + u64::try_from(imported.len()).unwrap_or_default();
    let overlap_end = current.min(end);
    if overlap_end > start {
        let existing = entries(connection, log, start..overlap_end)?;
        for (offset, existing_bytes) in existing.iter().enumerate() {
            if existing_bytes.as_slice() != imported[offset].bytes.as_slice() {
                let index = start + u64::try_from(offset).unwrap_or_default();
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the log {log} at entry {index} disagrees with its stored entry bundle",
                    ),
                });
            }
        }
    }
    Ok(current)
}

fn insert_imported_entries(
    transaction: &dyn Connection,
    log: &LogId,
    current: u64,
    imported: &[ImportedLogEntry],
) -> Result<()> {
    for (offset, item) in imported.iter().enumerate() {
        let index = current + u64::try_from(offset).unwrap_or_default();
        transaction.execute(
            "INSERT INTO log_entries (catalog, ecosystem, idx, entry) VALUES (?1, ?2, ?3, ?4)",
            params![log.catalog, log.ecosystem.as_str(), to_column(index), item.bytes.as_slice()],
        )?;
    }
    Ok(())
}

fn record_reimported_npm_versions(
    transaction: &dyn Connection,
    log: &LogId,
    imported: &[ImportedLogEntry],
) -> Result<()> {
    if log.ecosystem != Ecosystem::Npm {
        return Ok(());
    }
    let generation = super::catalog_generation(transaction, &log.catalog)? + 1;
    let mut recorded_any = false;
    for (name, version) in imported.iter().filter_map(|item| item.npm_version.as_ref()) {
        if version_is_live(transaction, &log.catalog, name, version)? {
            continue;
        }
        transaction.execute(
            "INSERT INTO removed_versions (catalog, ecosystem, name, version, generation)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT DO NOTHING",
            params![log.catalog, Ecosystem::Npm.as_str(), name, version, to_column(generation)],
        )?;
        recorded_any = true;
    }
    if recorded_any {
        super::advance_catalog(transaction, &log.catalog, generation)?;
    }
    Ok(())
}

fn version_is_live(
    connection: &dyn Connection,
    catalog: &str,
    name: &str,
    version: &str,
) -> Result<bool> {
    Ok(connection
        .query_optional(
            "SELECT 1 FROM versions
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
            params![catalog, Ecosystem::Npm.as_str(), name, version],
        )?
        .is_some())
}

fn parse_ecosystem(text: &str) -> Result<Ecosystem> {
    [Ecosystem::Npm, Ecosystem::Cargo, Ecosystem::Pypi, Ecosystem::Oci]
        .into_iter()
        .find(|ecosystem| ecosystem.as_str() == text)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("a log entry names the unknown ecosystem {text:?}"),
        })
}

fn to_column(index: u64) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}
