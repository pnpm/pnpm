use super::{
    Connection, HashMap, Mutex, RegistryError, Result, TokenInner, TokenKind, TokenRecord,
};
use crate::validation::{token_timestamp_from_sql, token_timestamp_to_sql};
use std::path::Path;

/// The token database connection every write of one store commits through.
///
/// `PRAGMA data_version` changes only when another connection commits, so a
/// change means another process, such as `pnpr add-user`, edited the tokens.
#[derive(Debug)]
pub(crate) struct TokenDatabase {
    connection: Connection,
    /// `data_version` when the tokens were last loaded.
    loaded_version: i64,
}

impl TokenDatabase {
    /// Open the database at `path`, migrating its schema, and load every token.
    pub(crate) fn open(path: &Path) -> Result<(Self, HashMap<String, TokenRecord>)> {
        let connection = Connection::open(path)?;
        init_tokens_schema(&connection)?;
        // A WAL read never waits for another process's write, so checking for
        // changes cannot stall a lookup.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let mut database = Self { connection, loaded_version: 0 };
        database.loaded_version = database.data_version()?;
        let tokens = load_all_tokens(&database.connection)?;
        Ok((database, tokens))
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Every token, if another connection committed since the last load.
    pub(crate) fn load_if_changed(&mut self) -> Result<Option<HashMap<String, TokenRecord>>> {
        let version = self.data_version()?;
        if version == self.loaded_version {
            return Ok(None);
        }
        // Read before loading: a commit in between loads again next time.
        let tokens = load_all_tokens(&self.connection)?;
        self.loaded_version = version;
        Ok(Some(tokens))
    }

    /// Whether another connection committed since the last load.
    pub(crate) fn has_changed(&self) -> Result<bool> {
        Ok(self.data_version()? != self.loaded_version)
    }

    fn data_version(&self) -> Result<i64> {
        Ok(self.connection.pragma_query_value(None, "data_version", |row| row.get(0))?)
    }
}

pub(super) fn lock_database(
    database: &Mutex<Option<TokenDatabase>>,
) -> std::sync::MutexGuard<'_, Option<TokenDatabase>> {
    database.lock().expect("TokenStore database mutex poisoned")
}

pub(super) fn sync_from_database(
    database: &mut Option<TokenDatabase>,
    inner: &Mutex<TokenInner>,
) -> Result<()> {
    let Some(database) = database else {
        return Ok(());
    };
    if let Some(tokens) = database.load_if_changed()? {
        *inner.lock().expect("TokenStore mutex poisoned") = TokenInner::from_tokens(tokens);
    }
    Ok(())
}

/// Write a consistent copy of the token database at `source` to `dest` with
/// [`write_private_atomic`](crate::write_private_atomic), including commits
/// still in its write-ahead log.
pub fn export_token_database(source: &Path, dest: &Path) -> Result<()> {
    let connection =
        Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let snapshot = connection.serialize(rusqlite::MAIN_DB)?;
    crate::write_private_atomic(dest, &snapshot)?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn create_private_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    Ok(())
}

pub(crate) fn init_tokens_schema(conn: &Connection) -> Result<()> {
    pnpr_sql::sqlite::ensure_schema(conn, "SQLite auth database")
}

fn decode_token_row(row: &rusqlite::Row<'_>) -> Result<(String, TokenRecord)> {
    let hash: String = row.get(0)?;
    let cidr_json: String = row.get(5)?;
    let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json)
        .map_err(|err| RegistryError::Internal {
            reason: format!("token {hash} has an unreadable cidr_whitelist: {err}"),
        })?;
    let expires_at: Option<i64> = row.get(7)?;
    let raw_kind: Option<String> = row.get(8)?;
    let kind = TokenKind::from_sql(raw_kind.as_deref(), &hash)?;
    Ok((
        hash,
        TokenRecord {
            owner_name: None,
            user_id: row.get(1)?,
            created_at: token_timestamp_from_sql(row.get(2)?),
            last_used_at: token_timestamp_from_sql(row.get(3)?),
            readonly: row.get::<_, i64>(4)? != 0,
            cidr_whitelist,
            name: row.get(6)?,
            expires_at: expires_at.map(token_timestamp_from_sql),
            kind,
        },
    ))
}

fn load_all_tokens(conn: &Connection) -> Result<HashMap<String, TokenRecord>> {
    let mut stmt = conn.prepare(
        "SELECT token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, kind
         FROM tokens",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = HashMap::new();
    while let Some(row) = rows.next()? {
        let (hash, record) = decode_token_row(row)?;
        out.insert(hash, record);
    }
    Ok(out)
}

pub(crate) fn delete_token(conn: &Connection, token_hash: &str) -> Result<()> {
    conn.execute("DELETE FROM tokens WHERE token_hash = ?1", rusqlite::params![token_hash])?;
    Ok(())
}

pub(crate) fn delete_user_tokens(conn: &Connection, user_id: &str) -> Result<usize> {
    let removed =
        conn.execute("DELETE FROM tokens WHERE user_id = ?1", rusqlite::params![user_id])?;
    Ok(removed)
}

pub(crate) fn delete_expired_tokens(conn: &Connection, now: u64) -> Result<usize> {
    let removed = conn.execute(
        "DELETE FROM tokens WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        rusqlite::params![token_timestamp_to_sql(now)],
    )?;
    Ok(removed)
}

pub(crate) fn insert_token(
    conn: &Connection,
    token_hash: &str,
    record: &TokenRecord,
) -> Result<()> {
    let cidr_json = serde_json::to_string(&record.cidr_whitelist)
        .expect("Vec<String> always serializes to JSON");
    conn.execute(
        "INSERT INTO tokens (token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            token_hash,
            record.user_id,
            token_timestamp_to_sql(record.created_at),
            token_timestamp_to_sql(record.last_used_at),
            i64::from(record.readonly),
            cidr_json,
            record.name.as_deref(),
            record.expires_at.map(token_timestamp_to_sql),
            record.kind.map(TokenKind::as_str),
        ],
    )?;
    Ok(())
}
