//! Networked-SQLite (libsql / Turso) auth backend.
//!
//! Backs both [`UserBackend`] and [`TokenBackend`] with one shared
//! database reached over the network, so several stateless pnpr
//! replicas observe a consistent set of users and tokens — the
//! prerequisite for running the registry as more than a single
//! instance.
//!
//! Unlike the local [`super::TokenStore`], there is no in-memory
//! mirror: a token issued on one replica must be resolvable on another,
//! so every [`TokenBackend::lookup`] hits the database. Reads are
//! therefore on the network hot path; an embedded-replica read cache is
//! the natural next optimization (see
//! [pnpm/pnpm#12199](https://github.com/pnpm/pnpm/issues/12199)).
//!
//! The SQL is identical to the local backend — the `tokens` table reuses
//! [`super::TOKENS_TABLE_SQL`] verbatim — so a database can be migrated
//! between the two. Users, which the local backend keeps in an htpasswd
//! file, live in a `users` table here ([`super::USERS_TABLE_SQL`]).

use super::{
    DEFAULT_BCRYPT_COST, TokenBackend, TokenRecord, UpsertOutcome, UserBackend, fresh_secret,
    hash_bcrypt, mint_token, sha256_hex, token_timestamp_from_sql, unix_seconds, validate_username,
    verify_bcrypt, verify_returning_user,
};
use crate::{
    config::{LibsqlSettings, MaxUsers},
    error::{RegistryError, Result},
};
use async_trait::async_trait;
use libsql::{Builder, Connection, Database, Error as LibsqlError, Row, params};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// The `tokens` columns selected (in order) by every token read, so
/// [`row_to_keyed_record`] can decode any of them the same way.
const TOKEN_COLUMNS: &str =
    "token_hash, username, created_at, last_used_at, readonly, cidr_whitelist";

/// Networked-SQLite auth backend. One [`Connection`] serves both record
/// stores; the [`Database`] handle is held only to keep the connection
/// alive.
pub struct LibsqlAuth {
    _db: Database,
    conn: Connection,
    /// Per-process secret seeding [`mint_token`]. Need not match other
    /// replicas: tokens are looked up by their stored hash, never
    /// re-derived, so each replica minting with its own secret is safe.
    secret: [u8; 32],
    counter: AtomicU64,
    max_users: MaxUsers,
}

impl std::fmt::Debug for LibsqlAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LibsqlAuth").finish_non_exhaustive()
    }
}

impl LibsqlAuth {
    /// Connect to the configured database and ensure the `users` and
    /// `tokens` tables exist. The fallible network step runs here, at
    /// startup, so an unreachable database fails fast rather than per
    /// request.
    ///
    /// With `replicaPath` set, the database is an embedded replica: an
    /// initial sync runs at build time and libsql keeps it current in
    /// the background (every `syncIntervalSecs`), so reads hit the local
    /// file. Without it, every read is a remote query against the
    /// primary — always fresh, but a network round-trip on the auth hot
    /// path.
    pub async fn connect(settings: &LibsqlSettings, max_users: MaxUsers) -> Result<Self> {
        let auth_token = settings.auth_token.clone().unwrap_or_default();
        let db = match &settings.replica_path {
            Some(path) => {
                let mut builder =
                    Builder::new_remote_replica(path, settings.url.clone(), auth_token);
                let interval = settings
                    .sync_interval_secs
                    .unwrap_or(LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS);
                if interval > 0 {
                    builder = builder.sync_interval(Duration::from_secs(interval));
                }
                builder.build().await?
            }
            None => Builder::new_remote(settings.url.clone(), auth_token).build().await?,
        };
        Self::from_database(db, max_users).await
    }

    /// Build the backend from an already-open [`Database`]. Shared by
    /// [`Self::connect`] and the local-database test setup.
    async fn from_database(db: Database, max_users: MaxUsers) -> Result<Self> {
        let conn = db.connect()?;
        init_schema(&conn).await?;
        Ok(Self { _db: db, conn, secret: fresh_secret(), counter: AtomicU64::new(0), max_users })
    }

    /// The bcrypt hash stored for `username`, or `None` when no such
    /// user exists.
    async fn stored_hash(&self, username: &str) -> Result<Option<String>> {
        let mut rows = self
            .conn
            .query("SELECT bcrypt_hash FROM users WHERE username = ?1", params![username])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get::<String>(0)?)),
            None => Ok(None),
        }
    }

    /// Current number of registered users — read under the
    /// registration cap, never on the hot path.
    async fn user_count(&self) -> Result<u64> {
        let mut rows = self.conn.query("SELECT COUNT(*) FROM users", ()).await?;
        let Some(row) = rows.next().await? else {
            return Err(missing_count_row());
        };
        let count: i64 = row.get(0)?;
        Ok(count.max(0) as u64)
    }
}

#[async_trait]
impl UserBackend for LibsqlAuth {
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;

        if let Some(stored) = self.stored_hash(username).await? {
            return verify_returning_user(username, password, stored).await;
        }

        // Brand-new user. The cheap pre-check avoids the (expensive) hash
        // when the cap is already full; the insert below re-checks the
        // cap atomically so it holds even under a concurrent burst.
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) if self.user_count().await? >= max => {
                return Err(RegistryError::TooManyUsers { max });
            }
            _ => {}
        }

        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        if matches!(self.max_users, MaxUsers::Unlimited) {
            let inserted = self
                .conn
                .execute(
                    "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                    params![username, hash],
                )
                .await;
            return match inserted {
                Ok(_) => Ok((UpsertOutcome::Created, username.to_string())),
                Err(err) if is_unique_violation(&err) => {
                    if let Some(stored) = self.stored_hash(username).await? {
                        return verify_returning_user(username, password, stored).await;
                    }
                    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
                }
                Err(err) => Err(err.into()),
            };
        }

        let mut can_retry_after_reconcile = true;
        loop {
            let tx = self.conn.transaction().await?;
            if let MaxUsers::Limited(max) = self.max_users {
                let sql_max = i64::try_from(max).map_err(|_| RegistryError::InvalidConfig {
                    reason: "backend.libsql auth max_users must fit a signed BIGINT".to_string(),
                })?;
                let updated = tx
                    .execute(
                        "UPDATE auth_counters SET value = value + 1
                         WHERE name = ?1 AND value < ?2",
                        params!["users", sql_max],
                    )
                    .await?;
                if updated == 0 {
                    tx.rollback().await?;
                    if can_retry_after_reconcile {
                        can_retry_after_reconcile = false;
                        if reconcile_user_counter_overcount(&self.conn).await? {
                            continue;
                        }
                    }
                    if let Some(stored) = self.stored_hash(username).await? {
                        return verify_returning_user(username, password, stored).await;
                    }
                    return Err(RegistryError::TooManyUsers { max });
                }
            }
            let inserted = tx
                .execute(
                    "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                    params![username, hash],
                )
                .await;
            match inserted {
                Ok(_) => {
                    tx.commit().await?;
                    return Ok((UpsertOutcome::Created, username.to_string()));
                }
                Err(err) if is_unique_violation(&err) => {
                    tx.rollback().await?;
                    if let Some(stored) = self.stored_hash(username).await? {
                        return verify_returning_user(username, password, stored).await;
                    }
                    return Err(RegistryError::Unauthenticated {
                        resource: format!("user {username:?}"),
                    });
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    async fn verify(&self, username: &str, password: &str) -> Result<Option<String>> {
        if validate_username(username).is_err() {
            return Ok(None);
        }

        let Some(stored) = self.stored_hash(username).await? else {
            return Ok(None);
        };
        let valid = verify_bcrypt(password.to_string(), stored).await?;
        Ok(valid.then(|| username.to_string()))
    }
}

#[async_trait]
impl TokenBackend for LibsqlAuth {
    async fn issue(&self, username: &str) -> Result<String> {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let raw = mint_token(&self.secret, nonce, username);
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_seconds() as i64;
        self.conn
            .execute(
                "INSERT INTO tokens
                 (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
             VALUES (?1, ?2, ?3, ?3, 0, '[]')",
                params![token_hash, username, now],
            )
            .await?;
        Ok(raw)
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let mut rows = self
            .conn
            .query("SELECT username FROM tokens WHERE token_hash = ?1", params![token_hash])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get::<String>(0)?)),
            None => Ok(None),
        }
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let query = format!("SELECT {TOKEN_COLUMNS} FROM tokens WHERE token_hash = ?1");
        let mut rows = self.conn.query(&query, params![key]).await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row_to_keyed_record(&row)?.1)),
            None => Ok(None),
        }
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        let query = format!("SELECT {TOKEN_COLUMNS} FROM tokens WHERE username = ?1");
        let mut rows = self.conn.query(&query, params![username]).await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_keyed_record(&row)?);
        }
        Ok(out)
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = self.find_by_key(key).await? else {
            return Ok(None);
        };
        self.conn.execute("DELETE FROM tokens WHERE token_hash = ?1", params![key]).await?;
        Ok(Some(record))
    }
}

async fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(super::USERS_TABLE_SQL, ()).await?;
    conn.execute(super::TOKENS_TABLE_SQL, ()).await?;
    conn.execute(super::TOKENS_INDEX_SQL, ()).await?;
    conn.execute(super::AUTH_COUNTERS_TABLE_SQL, ()).await?;
    ensure_user_counter(conn).await
}

async fn ensure_user_counter(conn: &Connection) -> Result<()> {
    let mut rows = conn.query("SELECT COUNT(*) FROM users", ()).await?;
    let Some(row) = rows.next().await? else {
        return Err(missing_count_row());
    };
    let count: i64 = row.get(0)?;
    let tx = conn.transaction().await?;
    let inserted = tx
        .execute("INSERT INTO auth_counters (name, value) VALUES (?1, ?2)", params!["users", count])
        .await;
    match inserted {
        Ok(_) => {}
        Err(err) if is_unique_violation(&err) => {}
        Err(err) => {
            tx.rollback().await?;
            return Err(err.into());
        }
    }
    tx.execute(
        "UPDATE auth_counters
         SET value = CASE WHEN value < ?2 THEN ?2 ELSE value END
         WHERE name = ?1",
        params!["users", count],
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn reconcile_user_counter_overcount(conn: &Connection) -> Result<bool> {
    let tx = conn.transaction().await?;
    let mut counter_rows =
        tx.query("SELECT value FROM auth_counters WHERE name = ?1", params!["users"]).await?;
    let Some(counter_row) = counter_rows.next().await? else {
        drop(counter_rows);
        tx.commit().await?;
        return Ok(false);
    };
    let counter: i64 = counter_row.get(0)?;
    drop(counter_rows);
    let mut count_rows = tx.query("SELECT COUNT(*) FROM users", ()).await?;
    let Some(count_row) = count_rows.next().await? else {
        return Err(missing_count_row());
    };
    let count: i64 = count_row.get(0)?;
    drop(count_rows);
    if counter <= count {
        tx.commit().await?;
        return Ok(false);
    }
    tx.execute(
        "UPDATE auth_counters SET value = ?2 WHERE name = ?1",
        params!["users", count.max(0)],
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

fn is_unique_violation(err: &LibsqlError) -> bool {
    match err {
        LibsqlError::SqliteFailure(code, message) => {
            *code == 19 || *code == 2067 || message.contains("UNIQUE constraint failed")
        }
        LibsqlError::RemoteSqliteFailure(_, code, message) => {
            *code == 19 || *code == 2067 || message.contains("UNIQUE constraint failed")
        }
        _ => false,
    }
}

fn missing_count_row() -> RegistryError {
    RegistryError::Internal { reason: "auth database COUNT(*) returned no rows".to_string() }
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
    let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json).unwrap_or_default();
    Ok((
        token_hash,
        TokenRecord {
            username,
            created_at: token_timestamp_from_sql(created_at),
            last_used_at: token_timestamp_from_sql(last_used_at),
            readonly: readonly != 0,
            cidr_whitelist,
        },
    ))
}

#[cfg(test)]
mod tests;
