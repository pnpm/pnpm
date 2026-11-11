use super::{super::super::TokenRecord, PostgresDatabase};
use crate::{
    TokenKind,
    validation::{token_timestamp_from_sql, token_timestamp_to_sql},
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
        sqlx::query(
            "INSERT INTO tokens
                (token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, kind)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(token_hash)
        .bind(&record.user_id)
        .bind(token_timestamp_to_sql(record.created_at))
        .bind(token_timestamp_to_sql(record.last_used_at))
        .bind(i64::from(record.readonly))
        .bind(cidr_json)
        .bind(record.name.as_deref())
        .bind(record.expires_at.map(token_timestamp_to_sql))
        .bind(record.kind.map(TokenKind::as_str))
        .execute(&mut *self.pool.acquire().await?)
        .await?;
        Ok(())
    }

    pub(super) async fn touch_token_impl(&self, token_hash: &str, now: u64) -> Result<()> {
        sqlx::query("UPDATE tokens SET last_used_at = $1 WHERE token_hash = $2")
            .bind(token_timestamp_to_sql(now))
            .bind(token_hash)
            .execute(&mut *self.pool.acquire().await?)
            .await?;
        Ok(())
    }

    pub(super) async fn find_token_impl(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        let row = sqlx::query(
            "SELECT user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, kind
             FROM tokens WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&mut *self.pool.acquire().await?)
        .await?;
        row.map(|row| token_record_from_row(&row, token_hash)).transpose()
    }

    pub(super) async fn find_owned_token_impl(
        &self,
        token_hash: &str,
    ) -> Result<Option<TokenRecord>> {
        let row = sqlx::query(
            "SELECT t.user_id, t.created_at, t.last_used_at, t.readonly, t.cidr_whitelist, t.name, t.expires_at, t.kind, u.username
             FROM tokens t JOIN users u ON u.id = t.user_id AND u.status = 'active' WHERE t.token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&mut *self.pool.acquire().await?)
        .await?;
        row.map(|row| {
            Ok(token_record_from_row(&row, token_hash)?
                .with_owner_name(row.try_get::<String, _>(8)?))
        })
        .transpose()
    }

    pub(super) async fn list_tokens_impl(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, TokenRecord)>> {
        let rows = sqlx::query(
            "SELECT token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, kind
             FROM tokens WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&mut *self.pool.acquire().await?)
        .await?;
        rows.into_iter()
            .map(|row| keyed_token_record_from_row(&row))
            .collect()
    }

    pub(super) async fn delete_token_impl(&self, token_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM tokens WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&mut *self.pool.acquire().await?)
            .await?;
        Ok(())
    }

    pub(super) async fn delete_expired_tokens_impl(&self, now: u64) -> Result<u64> {
        let result =
            sqlx::query("DELETE FROM tokens WHERE expires_at IS NOT NULL AND expires_at <= $1")
                .bind(token_timestamp_to_sql(now))
                .execute(&mut *self.pool.acquire().await?)
                .await?;
        Ok(result.rows_affected())
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
    let readonly: i64 = row.try_get(offset + 3)?;
    let name: Option<String> = row.try_get(offset + 5)?;
    let expires_at: Option<i64> = row.try_get(offset + 6)?;
    let raw_kind: Option<String> = row.try_get(offset + 7)?;
    let kind = TokenKind::from_sql(raw_kind.as_deref(), token_hash)?;
    Ok(TokenRecord {
        owner_name: None,
        user_id: row.try_get(offset)?,
        created_at: token_timestamp_from_sql(row.try_get(offset + 1)?),
        last_used_at: token_timestamp_from_sql(row.try_get(offset + 2)?),
        readonly: readonly != 0,
        cidr_whitelist,
        name,
        expires_at: expires_at.map(token_timestamp_from_sql),
        kind,
    })
}
