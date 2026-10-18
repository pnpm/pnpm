//! Second keys for `package_index` rows.
//!
//! A row is written under the integrity of the resource it was extracted
//! from. An alias makes the same row reachable under another integrity of the
//! same package, such as the hash of the tar a `.tgz` decodes to. Aliases live
//! apart from the rows, in a table pnpm v11 never reads, so a store shared
//! with it keeps a row layout that pnpm v11 understands.

use super::{Backend, Connection, HashMap, StoreIndex, StoreIndexError, queries::GET_MANY_CHUNK};

pub(super) const CREATE_ALIAS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS package_index_alias (
      key TEXT PRIMARY KEY,
      target TEXT NOT NULL
    ) WITHOUT ROWID;
";

impl StoreIndex {
    /// The row key `key` is an alias of, if it is one.
    pub fn alias_target(&self, key: &str) -> Result<Option<String>, StoreIndexError> {
        Ok(self
            .alias_targets(&[key.to_string()])?
            .remove(key))
    }

    /// `alias → target` for every key of `keys` that is an alias.
    pub(super) fn alias_targets(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, String>, StoreIndexError> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        match &self.backend {
            Backend::Sqlite { conn, .. } => sqlite_alias_targets(conn, keys),
            Backend::Files(files) => Ok(files
                .aliases()
                .get_many(keys)?
                .into_iter()
                .filter_map(|(key, target)| Some((key, String::from_utf8(target).ok()?)))
                .collect()),
        }
    }

    /// Record each `(alias, target)` pair, replacing an earlier target.
    pub fn set_aliases(&mut self, entries: &[(String, String)]) -> Result<(), StoreIndexError> {
        if entries.is_empty() {
            return Ok(());
        }
        let conn = match &mut self.backend {
            Backend::Sqlite { conn, .. } => conn,
            Backend::Files(files) => {
                let encoded: Vec<(String, Vec<u8>)> = entries
                    .iter()
                    .map(|(alias, target)| (alias.clone(), target.as_bytes().to_vec()))
                    .collect();
                return files.aliases().set_many(&encoded);
            }
        };
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|source| StoreIndexError::Write { source })?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO package_index_alias (key, target) VALUES (?1, ?2)",
                )
                .map_err(|source| StoreIndexError::Write { source })?;
            for (alias, target) in entries {
                stmt.execute(rusqlite::params![alias, target])
                    .map_err(|source| StoreIndexError::Write { source })?;
            }
        }
        tx.commit()
            .map_err(|source| StoreIndexError::Write { source })
    }
}

fn sqlite_alias_targets(
    conn: &Connection,
    keys: &[String],
) -> Result<HashMap<String, String>, StoreIndexError> {
    let mut out = HashMap::new();
    for chunk in keys.chunks(GET_MANY_CHUNK) {
        // Only the placeholder list is interpolated; the keys are bound.
        let placeholders = std::iter::repeat_n("?", chunk.len()).collect::<Vec<_>>().join(",");
        let sql =
            format!("SELECT key, target FROM package_index_alias WHERE key IN ({placeholders})");
        let mut stmt = match conn.prepare_cached(&sql) {
            Ok(stmt) => stmt,
            // A read-only index without the alias table holds no aliases.
            Err(error) if is_missing_alias_table(&error) => return Ok(out),
            Err(source) => return Err(StoreIndexError::Read { source }),
        };
        let params = rusqlite::params_from_iter(chunk.iter().map(String::as_str));
        let rows = stmt
            .query_map(params, |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|source| StoreIndexError::Read { source })?;
        for row in rows {
            let (alias, target) = row.map_err(|source| StoreIndexError::Read { source })?;
            out.insert(alias, target);
        }
    }
    Ok(out)
}

fn is_missing_alias_table(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(_, Some(message)) if message.contains("no such table"))
}

#[cfg(test)]
mod tests;
