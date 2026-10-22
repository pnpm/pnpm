use super::{LibsqlAuth, RegistryError, Result, Row, TokenBackend, TokenRecord, params};
use crate::{
    NewToken,
    token_store::{mint_token, sha256_hex, unix_seconds},
    validation::{
        token_scopes_from_sql, token_scopes_to_sql, token_timestamp_from_sql,
        token_timestamp_to_sql,
    },
};
use async_trait::async_trait;
use std::sync::atomic::Ordering;

/// The `tokens` columns selected (in order) by every token read, so
/// [`row_to_keyed_record`] can decode any of them the same way.
const TOKEN_COLUMNS: &str = "token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind";

/// [`TOKEN_COLUMNS`] of the `tokens` table aliased `t`, for the reads that
/// join `users` to require a stored owner.
const OWNED_TOKEN_COLUMNS: &str = "t.token_hash, t.username, t.created_at, t.last_used_at, t.readonly, t.cidr_whitelist, t.name, t.expires_at, t.scopes, t.kind";

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
            scopes: new_token.scopes,
            kind: new_token.kind,
        };
        self.insert_token(token_hash, &record).await?;
        self.sync_replica().await?;
        Ok((raw, record))
    }

    async fn issue(&self, username: &str) -> Result<String> {
        let (raw, _) = self.issue_token(username, NewToken::default()).await?;
        Ok(raw)
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        let sql_now = token_timestamp_to_sql(now);
        // Not synced back into a replica: the background sync carries the
        // timestamp. It runs under the read deadline: losing a timestamp to
        // it is harmless.
        self.pool.read(|conn| async move {
            conn.execute(
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
        self.pool.read(|conn| async move {
            let mut rows = conn
                .query(
                    "SELECT t.username, t.expires_at FROM tokens t JOIN users u ON u.username = t.username
                     WHERE t.token_hash = ?1",
                    params![token_hash],
                )
                .await?;
            match rows.next().await? {
                Some(row) => unexpired_owner(&row, now),
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
        self.pool.read(|conn| async move {
            let mut rows = conn.query(&query, params![token_hash]).await?;
            match rows.next().await? {
                Some(row) => Ok(unexpired(row_to_keyed_record(&row)?.1, now)),
                None => Ok(None),
            }
        })
        .await
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let query = format!("SELECT {TOKEN_COLUMNS} FROM tokens WHERE token_hash = ?1");
        self.pool.read(|conn| async move {
            let mut rows = conn.query(&query, params![key]).await?;
            match rows.next().await? {
                Some(row) => Ok(Some(row_to_keyed_record(&row)?.1)),
                None => Ok(None),
            }
        })
        .await
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        let query = format!("SELECT {TOKEN_COLUMNS} FROM tokens WHERE username = ?1");
        self.pool.read(|conn| async move {
            let mut rows = conn.query(&query, params![username]).await?;
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
        self.pool.write(|conn| async move {
            conn.execute("DELETE FROM tokens WHERE token_hash = ?1", params![key]).await?;
            Ok(())
        })
        .await?;
        self.sync_replica().await?;
        Ok(Some(record))
    }

    async fn revoke_all_for_user(&self, username: &str) -> Result<usize> {
        let removed = self.pool.write(|conn| async move {
            Ok(conn.execute("DELETE FROM tokens WHERE username = ?1", params![username]).await?)
        })
        .await?;
        // Another instance may have revoked the tokens this replica still
        // holds, so a zero-row delete does not settle whether to pull.
        if removed > 0 || self.replica_holds_tokens(username).await? {
            self.sync_replica().await?;
        }
        Ok(usize::try_from(removed).unwrap_or(usize::MAX))
    }

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
    }
}

impl LibsqlAuth {
    async fn insert_token(&self, token_hash: String, record: &TokenRecord) -> Result<()> {
        let cidr_json = serde_json::to_string(&record.cidr_whitelist)
            .expect("Vec<String> always serializes to JSON");
        let scopes_json = token_scopes_to_sql(record.scopes.as_deref());
        self.pool.write(|conn| async move {
            conn.execute(
                "INSERT INTO tokens
                     (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, scopes, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    token_hash,
                    record.username.clone(),
                    token_timestamp_to_sql(record.created_at),
                    token_timestamp_to_sql(record.last_used_at),
                    i64::from(record.readonly),
                    cidr_json,
                    record.name.as_deref(),
                    record.expires_at.map(token_timestamp_to_sql),
                    scopes_json.as_deref(),
                    record.kind.as_deref(),
                ],
            )
            .await?;
            Ok(())
        })
        .await
    }

    /// Whether the embedded replica still lists a token of `username`;
    /// `false` without a replica, since the pool then reads the primary.
    async fn replica_holds_tokens(&self, username: &str) -> Result<bool> {
        if !self.is_replica {
            return Ok(false);
        }
        self.pool.read(|conn| async move {
            let mut rows =
                conn.query("SELECT 1 FROM tokens WHERE username = ?1 LIMIT 1", params![username])
                    .await?;
            Ok(rows.next().await?.is_some())
        })
        .await
    }
}

/// The owner of a token row selecting `username, expires_at`, unless the token
/// expired by `now`.
fn unexpired_owner(row: &Row, now: u64) -> Result<Option<String>> {
    let username: String = row.get(0)?;
    let expires_at: Option<i64> = row.get(1)?;
    let expired = expires_at
        .map(token_timestamp_from_sql)
        .is_some_and(|exp| exp <= now);
    Ok((!expired).then_some(username))
}

/// `record`, unless it expired by `now`.
fn unexpired(record: TokenRecord, now: u64) -> Option<TokenRecord> {
    let expired = record.expires_at.is_some_and(|exp| exp <= now);
    (!expired).then_some(record)
}

/// Decode a row selecting [`TOKEN_COLUMNS`] into its `(token_hash,
/// record)` pair.
fn row_to_keyed_record(row: &Row) -> Result<(String, TokenRecord)> {
    let token_hash: String = row.get(0)?;
    let cidr_json: String = row.get(5)?;
    let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json)
        .map_err(|err| RegistryError::Internal {
            reason: format!("token {token_hash} has an unreadable cidr_whitelist: {err}"),
        })?;
    let scopes_json: Option<String> = row.get(8)?;
    let scopes = token_scopes_from_sql(scopes_json.as_deref(), &token_hash)?;
    let expires_at: Option<i64> = row.get(7)?;
    Ok((
        token_hash,
        TokenRecord {
            username: row.get(1)?,
            created_at: token_timestamp_from_sql(row.get(2)?),
            last_used_at: token_timestamp_from_sql(row.get(3)?),
            readonly: row.get::<i64>(4)? != 0,
            cidr_whitelist,
            name: row.get(6)?,
            expires_at: expires_at.map(token_timestamp_from_sql),
            scopes,
            kind: row.get(9)?,
        },
    ))
}
