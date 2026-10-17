use super::{super::super::TokenRecord, PostgresDatabase};
use crate::validation::{
    token_scopes_from_sql, token_scopes_to_sql, token_timestamp_from_sql, token_timestamp_to_sql,
};
use pnpr_error::{RegistryError, Result};
use sqlx::Row;

impl PostgresDatabase {
    pub(super) async fn insert_token_impl(
        &self,
        token_hash: &str,
        record: &TokenRecord,
    ) -> Result<()> {
        let cidr_json = serde_json::to_string(&record.cidr_whitelist)
            .expect("Vec<String> always serializes to JSON");
        let scopes_json = token_scopes_to_sql(record.scopes.as_deref());
        sqlx::query(
            "INSERT INTO tokens
                (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(token_hash)
        .bind(&record.username)
        .bind(token_timestamp_to_sql(record.created_at))
        .bind(token_timestamp_to_sql(record.last_used_at))
        .bind(i16::from(record.readonly))
        .bind(cidr_json)
        .bind(record.name.as_deref())
        .bind(record.expires_at.map(token_timestamp_to_sql))
        .bind(scopes_json)
        .bind(record.kind.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(super) async fn touch_token_impl(&self, token_hash: &str, now: u64) -> Result<()> {
        sqlx::query("UPDATE tokens SET last_used_at = $1 WHERE token_hash = $2")
            .bind(token_timestamp_to_sql(now))
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(super) async fn find_token_impl(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        let row = sqlx::query(
            "SELECT username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind
             FROM tokens WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| token_record_from_row(&row, token_hash)).transpose()
    }

    pub(super) async fn find_owned_token_impl(
        &self,
        token_hash: &str,
    ) -> Result<Option<TokenRecord>> {
        let row = sqlx::query(
            "SELECT t.username, t.created_at, t.last_used_at, t.readonly, t.cidr_whitelist, t.name, t.expires_at, t.scopes, t.kind
             FROM tokens t JOIN users u ON u.username = t.username WHERE t.token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| token_record_from_row(&row, token_hash)).transpose()
    }

    pub(super) async fn list_tokens_impl(
        &self,
        username: &str,
    ) -> Result<Vec<(String, TokenRecord)>> {
        let rows = sqlx::query(
            "SELECT token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind
             FROM tokens WHERE username = $1",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| keyed_token_record_from_row(&row))
            .collect()
    }

    pub(super) async fn delete_token_impl(&self, token_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM tokens WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn keyed_token_record_from_row(row: &sqlx::postgres::PgRow) -> Result<(String, TokenRecord)> {
    let token_hash: String = row.try_get(0)?;
    let record = token_record_from_offset(row, 1, &token_hash)?;
    Ok((token_hash, record))
}

fn token_record_from_row(row: &sqlx::postgres::PgRow, token_hash: &str) -> Result<TokenRecord> {
    token_record_from_offset(row, 0, token_hash)
}

fn token_record_from_offset(
    row: &sqlx::postgres::PgRow,
    offset: usize,
    token_hash: &str,
) -> Result<TokenRecord> {
    let cidr_json: String = row.try_get(offset + 4)?;
    let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json)
        .map_err(|err| RegistryError::Internal {
            reason: format!("token {token_hash} has an unreadable cidr_whitelist: {err}"),
        })?;
    let readonly: i16 = row.try_get(offset + 3)?;
    let name: Option<String> = row.try_get(offset + 5)?;
    let expires_at: Option<i64> = row.try_get(offset + 6)?;
    let scopes_json: Option<String> = row.try_get(offset + 7)?;
    let kind: Option<String> = row.try_get(offset + 8)?;
    let scopes = token_scopes_from_sql(scopes_json.as_deref(), token_hash)?;
    Ok(TokenRecord {
        username: row.try_get(offset)?,
        created_at: token_timestamp_from_sql(row.try_get(offset + 1)?),
        last_used_at: token_timestamp_from_sql(row.try_get(offset + 2)?),
        readonly: readonly != 0,
        cidr_whitelist,
        name,
        expires_at: expires_at.map(token_timestamp_from_sql),
        scopes,
        kind,
    })
}
