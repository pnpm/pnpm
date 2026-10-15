use super::{Connection, HashMap, RegistryError, Result, TokenRecord};
use crate::validation::{token_timestamp_from_sql, token_timestamp_to_sql};

/// `tokens` table DDL — shared by every SQL-backed auth store so the
/// backends store the same shape and records can be moved between them.
pub(crate) const TOKENS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS tokens (
    token_hash      CHAR(64) PRIMARY KEY,
    username        VARCHAR(255) NOT NULL,
    created_at      BIGINT NOT NULL,
    last_used_at    BIGINT NOT NULL,
    readonly        SMALLINT NOT NULL DEFAULT 0,
    cidr_whitelist  VARCHAR(4096) NOT NULL DEFAULT '[]',
    name            VARCHAR(255),
    expires_at      BIGINT
)";

pub(crate) const TOKENS_INDEX_SQL: &str =
    "CREATE INDEX IF NOT EXISTS tokens_username ON tokens(username)";

pub(crate) fn init_tokens_schema(conn: &Connection) -> Result<()> {
    conn.execute(TOKENS_TABLE_SQL, [])?;
    conn.execute(TOKENS_INDEX_SQL, [])?;
    let _ = conn.execute("ALTER TABLE tokens ADD COLUMN name VARCHAR(255)", []);
    let _ = conn.execute("ALTER TABLE tokens ADD COLUMN expires_at BIGINT", []);
    Ok(())
}

fn decode_token_row(row: &rusqlite::Row<'_>) -> Result<(String, TokenRecord)> {
    let hash: String = row.get(0)?;
    let username: String = row.get(1)?;
    let created_at: i64 = row.get(2)?;
    let last_used_at: i64 = row.get(3)?;
    let readonly: i64 = row.get(4)?;
    let cidr_json: String = row.get(5)?;
    let name: Option<String> = row.get(6)?;
    let expires_at: Option<i64> = row.get(7)?;
    let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json)
        .map_err(|err| RegistryError::Internal {
            reason: format!("token {hash} has an unreadable cidr_whitelist: {err}"),
        })?;
    Ok((
        hash,
        TokenRecord {
            username,
            created_at: token_timestamp_from_sql(created_at),
            last_used_at: token_timestamp_from_sql(last_used_at),
            readonly: readonly != 0,
            cidr_whitelist,
            name,
            expires_at: expires_at.map(token_timestamp_from_sql),
        },
    ))
}

pub(crate) fn load_all_tokens(conn: &Connection) -> Result<HashMap<String, TokenRecord>> {
    let mut stmt = conn.prepare(
        "SELECT token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at
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

pub(crate) fn insert_token(
    conn: &Connection,
    token_hash: &str,
    record: &TokenRecord,
) -> Result<()> {
    let cidr_json = serde_json::to_string(&record.cidr_whitelist)
        .expect("Vec<String> always serializes to JSON");
    conn.execute(
        "INSERT INTO tokens (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            token_hash,
            record.username,
            token_timestamp_to_sql(record.created_at),
            token_timestamp_to_sql(record.last_used_at),
            i64::from(record.readonly),
            cidr_json,
            record.name.as_deref(),
            record.expires_at.map(|exp| exp as i64),
        ],
    )?;
    Ok(())
}
