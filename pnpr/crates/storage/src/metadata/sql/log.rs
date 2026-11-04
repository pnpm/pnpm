//! The sequenced entries of each catalog's transparency logs, one log per
//! catalog and ecosystem.

use super::{PackageKey, adapter::Connection, packument::ORDINARY};
use crate::transparency_log::LogId;
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
         WHERE catalog = ?1 AND ecosystem = ?2 AND idx >= ?3 AND idx < ?4
         ORDER BY idx",
        params![
            log.catalog,
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
        "SELECT COALESCE(MAX(idx) + 1, 0) FROM log_entries WHERE catalog = ?1 AND ecosystem = ?2",
        params![log.catalog, log.ecosystem.as_str()],
    )?;
    Ok(u64::try_from(row.get::<i64>(0)?).unwrap_or_default())
}

/// The tree `log` was integrated to: its size and root.
pub(super) fn head(connection: &dyn Connection, log: &LogId) -> Result<Option<(u64, Vec<u8>)>> {
    let row = connection.query_optional(
        "SELECT size, root FROM log_heads WHERE catalog = ?1 AND ecosystem = ?2",
        params![log.catalog, log.ecosystem.as_str()],
    )?;
    row.map(|mut row| Ok((u64::try_from(row.get::<i64>(0)?).unwrap_or_default(), row.get(1)?)))
        .transpose()
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
    transaction.execute(
        "INSERT INTO log_heads (catalog, ecosystem, size, root) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (catalog, ecosystem) DO UPDATE SET size = excluded.size, root = excluded.root
         WHERE excluded.size > log_heads.size",
        params![log.catalog, log.ecosystem.as_str(), to_column(size), root],
    )?;
    transaction.commit()
}

/// Every log with a sequenced entry, with the number of its entries.
pub(super) fn sizes(connection: &dyn Connection) -> Result<Vec<(LogId, u64)>> {
    let rows = connection.query(
        "SELECT catalog, ecosystem, MAX(idx) + 1 FROM log_entries GROUP BY catalog, ecosystem",
        params![],
    )?;
    rows.into_iter()
        .map(|mut row| {
            let catalog: String = row.get(0)?;
            let ecosystem: String = row.get(1)?;
            let ecosystem = parse_ecosystem(&ecosystem)?;
            let size = u64::try_from(row.get::<i64>(2)?).unwrap_or_default();
            Ok((LogId { catalog, ecosystem }, size))
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
    let (catalog, ecosystem, name) = key.params();
    let row = connection.query_optional(
        "SELECT log_index, digest FROM release_records
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4
           AND revision_key = ?5",
        params![catalog, ecosystem, name, version, ORDINARY],
    )?;
    row.map(|mut row| {
        let index = u64::try_from(row.get::<i64>(0)?).unwrap_or_default();
        Ok((index, row.get(1)?))
    })
    .transpose()
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
