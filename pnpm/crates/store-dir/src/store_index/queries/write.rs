use super::{
    super::{
        PackageFilesIndex,
        StoreIndexError,
    },
    EncodingErrorPolicy,
};
use rusqlite::Connection;

pub(super) fn encode_index_entry(
    key: String,
    value: &PackageFilesIndex,
    policy: EncodingErrorPolicy,
) -> Result<Option<(String, Vec<u8>)>, StoreIndexError> {
    match crate::msgpackr_records::encode_package_files_index(value) {
        Ok(encoded) => Ok(Some((key, encoded))),
        Err(source) => match policy {
            EncodingErrorPolicy::Return => Err(StoreIndexError::Encode { source }),
            EncodingErrorPolicy::Skip => {
                tracing::warn!(
                    target: "pacquet::store_index",
                    ?key,
                    error = ?source,
                    "failed to encode package_index row; skipping",
                );
                Ok(None)
            }
        },
    }
}

pub(super) fn write_many_transaction(
    conn: &mut Connection,
    entries: &[(String, Vec<u8>)],
) -> Result<(), StoreIndexError> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|source| StoreIndexError::Write { source })?;
    write_many_entries(&tx, entries)?;
    tx.commit()
        .map_err(|source| StoreIndexError::Write { source })
}

fn write_many_entries(
    tx: &rusqlite::Transaction<'_>,
    entries: &[(String, Vec<u8>)],
) -> Result<(), StoreIndexError> {
    let mut stmt = tx
        .prepare_cached("INSERT OR REPLACE INTO package_index (key, data) VALUES (?1, ?2)")
        .map_err(|source| StoreIndexError::Write { source })?;
    for (key, buf) in entries {
        stmt.execute(rusqlite::params![key, buf])
            .map_err(|source| StoreIndexError::Write { source })?;
    }
    Ok(())
}
