use super::{Connection, HashMap, RegistryError, Result, TokenRecord};
use crate::validation::{
    token_scopes_from_sql, token_scopes_to_sql, token_timestamp_from_sql, token_timestamp_to_sql,
};

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
    let scopes_json: Option<String> = row.get(8)?;
    let scopes = token_scopes_from_sql(scopes_json.as_deref(), &hash)?;
    let expires_at: Option<i64> = row.get(7)?;
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
            scopes,
            kind: row.get(9)?,
        },
    ))
}

pub(crate) fn load_all_tokens(conn: &Connection) -> Result<HashMap<String, TokenRecord>> {
    let mut stmt = conn.prepare(
        "SELECT token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind
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
    let scopes_json = token_scopes_to_sql(record.scopes.as_deref());
    conn.execute(
        "INSERT INTO tokens (token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            token_hash,
            record.user_id,
            token_timestamp_to_sql(record.created_at),
            token_timestamp_to_sql(record.last_used_at),
            i64::from(record.readonly),
            cidr_json,
            record.name.as_deref(),
            record.expires_at.map(token_timestamp_to_sql),
            scopes_json.as_deref(),
            record.kind.as_deref(),
        ],
    )?;
    Ok(())
}
