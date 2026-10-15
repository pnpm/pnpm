use super::{
    LibsqlAuth, RegistryError, Result, Row, TokenBackend, TokenRecord, params, with_auth_timeout,
};
use crate::{
    NewToken,
    token_store::{mint_token, sha256_hex, unix_seconds},
    validation::{token_timestamp_from_sql, token_timestamp_to_sql},
};
use async_trait::async_trait;
use std::sync::atomic::Ordering;

/// The `tokens` columns selected (in order) by every token read, so
/// [`row_to_keyed_record`] can decode any of them the same way.
const TOKEN_COLUMNS: &str =
    "token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at";

/// [`TOKEN_COLUMNS`] of the `tokens` table aliased `t`, for the reads that
/// join `users` to require a stored owner.
const OWNED_TOKEN_COLUMNS: &str = "t.token_hash, t.username, t.created_at, t.last_used_at, t.readonly, t.cidr_whitelist, t.name, t.expires_at";

#[async_trait]
impl TokenBackend for LibsqlAuth {
    async fn issue_token(
        &self,
        username: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let raw = mint_token(&self.secret, nonce, username);
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_seconds();
        let record = TokenRecord {
            username: username.to_string(),
            created_at: now,
            last_used_at: now,
            readonly: new_token.readonly,
            cidr_whitelist: new_token.cidr_whitelist,
            name: new_token.name,
            expires_at: new_token.expires_at,
        };
        let cidr_json = serde_json::to_string(&record.cidr_whitelist)
            .expect("Vec<String> always serializes to JSON");
        self.conn
            .execute(
                "INSERT INTO tokens
                     (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    token_hash,
                    record.username.clone(),
                    token_timestamp_to_sql(record.created_at),
                    token_timestamp_to_sql(record.last_used_at),
                    i64::from(record.readonly),
                    cidr_json,
                    record.name.as_deref(),
                    record.expires_at.map(token_timestamp_to_sql),
                ],
            )
            .await?;
        Ok((raw, record))
    }

    async fn issue(&self, username: &str) -> Result<String> {
        let (raw, _) = self.issue_token(username, NewToken::default()).await?;
        Ok(raw)
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        let sql_now = token_timestamp_to_sql(now);
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            self.conn.execute(
                "UPDATE tokens SET last_used_at = ?1 WHERE token_hash = ?2",
                params![sql_now, key],
            )
            .await?;
            Ok(())
        })
        .await
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_seconds();
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self
                .conn
                .query(
                    "SELECT t.username, t.expires_at FROM tokens t JOIN users u ON u.username = t.username
                     WHERE t.token_hash = ?1",
                    params![token_hash],
                )
                .await?;
            match rows.next().await? {
                Some(row) => {
                    let username: String = row.get(0)?;
                    let expires_at: Option<i64> = row.get(1)?;
                    if expires_at.map(token_timestamp_from_sql).is_some_and(|exp| exp <= now) {
                        Ok(None)
                    } else {
                        Ok(Some(username))
                    }
                }
                None => Ok(None),
            }
        })
        .await
    }

    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let query = format!(
            "SELECT {OWNED_TOKEN_COLUMNS} FROM tokens t JOIN users u ON u.username = t.username
             WHERE t.token_hash = ?1",
        );
        let now = unix_seconds();
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self.conn.query(&query, params![token_hash]).await?;
            match rows.next().await? {
                Some(row) => {
                    let record = row_to_keyed_record(&row)?.1;
                    if record.expires_at.is_some_and(|exp| exp <= now) {
                        Ok(None)
                    } else {
                        Ok(Some(record))
                    }
                }
                None => Ok(None),
            }
        })
        .await
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let query = format!("SELECT {TOKEN_COLUMNS} FROM tokens WHERE token_hash = ?1");
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self.conn.query(&query, params![key]).await?;
            match rows.next().await? {
                Some(row) => Ok(Some(row_to_keyed_record(&row)?.1)),
                None => Ok(None),
            }
        })
        .await
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        let query = format!("SELECT {TOKEN_COLUMNS} FROM tokens WHERE username = ?1");
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self.conn.query(&query, params![username]).await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(row_to_keyed_record(&row)?);
            }
            Ok(out)
        })
        .await
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = self.find_by_key(key).await? else {
            return Ok(None);
        };
        self.conn.execute("DELETE FROM tokens WHERE token_hash = ?1", params![key]).await?;
        Ok(Some(record))
    }

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
    }
}

/// Decode a row selecting [`TOKEN_COLUMNS`] into its `(token_hash,
/// record)` pair.
fn row_to_keyed_record(row: &Row) -> Result<(String, TokenRecord)> {
    let token_hash: String = row.get(0)?;
    let username: String = row.get(1)?;
    let created_at: i64 = row.get(2)?;
    let last_used_at: i64 = row.get(3)?;
    let readonly: i64 = row.get(4)?;
    let cidr_json: String = row.get(5)?;
    let name: Option<String> = row.get(6)?;
    let expires_at: Option<i64> = row.get(7)?;
    let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json)
        .map_err(|err| RegistryError::Internal {
            reason: format!("token {token_hash} has an unreadable cidr_whitelist: {err}"),
        })?;
    Ok((
        token_hash,
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
