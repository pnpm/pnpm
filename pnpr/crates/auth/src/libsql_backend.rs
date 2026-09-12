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
//! [`super::token_store::TOKENS_TABLE_SQL`] verbatim — so a database can be migrated
//! between the two. Users, which the local backend keeps in an htpasswd
//! file, live in a `users` table here ([`super::USERS_TABLE_SQL`]).

use super::token_store::{mint_token, unix_seconds};
mod schema;
use schema::{
    claim_user_counter_slot, init_schema, is_unique_violation, missing_count_row,
    reconcile_user_counter_overcount, retry_database_conflicts,
};

use super::{
    DEFAULT_BCRYPT_COST, TokenBackend, TokenRecord, UpsertOutcome, UserBackend, fresh_secret,
    hash_bcrypt, sha256_hex, token_timestamp_from_sql, validate_username, verify_returning_user,
    with_auth_timeout,
};
use async_trait::async_trait;
use libsql::{
    Builder, Connection, Database, Error as LibsqlError, Row, TransactionBehavior, params,
};
use pnpr_config::{LibsqlSettings, MaxUsers};
use pnpr_error::{RegistryError, Result};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// The `tokens` columns selected (in order) by every token read, so
/// [`row_to_keyed_record`] can decode any of them the same way.
const TOKEN_COLUMNS: &str =
    "token_hash, username, created_at, last_used_at, readonly, cidr_whitelist";

/// Deadline for request-path auth reads, beyond which a stalled endpoint
/// surfaces [`RegistryError::AuthDatabaseTimeout`] rather than hanging.
const DEFAULT_AUTH_TIMEOUT: Duration = Duration::from_secs(30);

/// Deadline for the one-time startup connect and schema setup; more
/// generous than [`DEFAULT_AUTH_TIMEOUT`] for cold remote DDL.
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_mins(5);

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
    registration_lock: tokio::sync::Mutex<()>,
    /// Deadline for each request-path auth read.
    timeout: Duration,
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
                with_auth_timeout(DEFAULT_STARTUP_TIMEOUT, Box::pin(builder.build())).await?
            }
            None => {
                with_auth_timeout(
                    DEFAULT_STARTUP_TIMEOUT,
                    Box::pin(Builder::new_remote(settings.url.clone(), auth_token).build()),
                )
                .await?
            }
        };
        Self::from_database(db, max_users).await
    }

    /// Build the backend from an already-open [`Database`]. Shared by
    /// [`Self::connect`] and the local-database test setup.
    async fn from_database(db: Database, max_users: MaxUsers) -> Result<Self> {
        let conn = db.connect()?;
        with_auth_timeout(DEFAULT_STARTUP_TIMEOUT, init_schema(&conn)).await?;
        Ok(Self {
            _db: db,
            conn,
            secret: fresh_secret(),
            counter: AtomicU64::new(0),
            max_users,
            registration_lock: tokio::sync::Mutex::new(()),
            timeout: DEFAULT_AUTH_TIMEOUT,
        })
    }

    /// The bcrypt hash stored for `username`, or `None` when no such
    /// user exists.
    async fn stored_hash(&self, username: &str) -> Result<Option<String>> {
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self
                .conn
                .query("SELECT bcrypt_hash FROM users WHERE username = ?1", params![username])
                .await?;
            match rows.next().await? {
                Some(row) => Ok(Some(row.get::<String>(0)?)),
                None => Ok(None),
            }
        })
        .await
    }

    /// Current number of registered users — read under the
    /// registration cap, never on the hot path.
    async fn user_count(&self) -> Result<u64> {
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self.conn.query("SELECT COUNT(*) FROM users", ()).await?;
            let Some(row) = rows.next().await? else {
                return Err(missing_count_row());
            };
            let count: i64 = row.get(0)?;
            Ok(count.max(0) as u64)
        })
        .await
    }
}

#[async_trait]
impl UserBackend for LibsqlAuth {
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        let hash = tokio::sync::OnceCell::new();
        retry_database_conflicts(|| self.add_or_login_attempt(username, password, &hash)).await
    }
}

impl LibsqlAuth {
    async fn add_or_login_attempt(
        &self,
        username: &str,
        password: &str,
        hash: &tokio::sync::OnceCell<String>,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;

        if let Some(stored) = self.stored_hash(username).await? {
            return verify_returning_user(username, password, stored).await;
        }

        // Brand-new user. The cheap pre-check avoids the (expensive) hash
        // when the cap is already full; the insert below re-checks the
        // cap atomically so it holds even under a concurrent burst.
        self.check_registration_allowed().await?;

        let hash =
            hash.get_or_try_init(|| hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST)).await?;
        if matches!(self.max_users, MaxUsers::Unlimited) {
            return self.insert_uncapped_user(username, password, hash).await;
        }

        let _registration_guard = self.registration_lock.lock().await;
        self.insert_capped_user(username, password, hash).await
    }

    /// Whether a new user may register at all, judged before the bcrypt cost
    /// is paid. The insert re-checks the cap atomically.
    async fn check_registration_allowed(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) if self.user_count().await? >= max => {
                Err(RegistryError::TooManyUsers { max })
            }
            _ => Ok(()),
        }
    }

    /// Insert a user into an uncapped store. A concurrent insert of the same
    /// name turns the registration into a login.
    async fn insert_uncapped_user(
        &self,
        username: &str,
        password: &str,
        hash: &str,
    ) -> Result<(UpsertOutcome, String)> {
        let inserted = self
            .conn
            .execute(
                "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                params![username, hash],
            )
            .await;
        match inserted {
            Ok(_) => Ok((UpsertOutcome::Created, username.to_string())),
            Err(err) if is_unique_violation(&err) => {
                self.login_after_lost_insert(username, password).await
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Insert a user under a cap, claiming a counter slot in the same
    /// transaction so the cap holds under a concurrent burst.
    async fn insert_capped_user(
        &self,
        username: &str,
        password: &str,
        hash: &str,
    ) -> Result<(UpsertOutcome, String)> {
        let mut can_retry_after_reconcile = true;
        loop {
            let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate).await?;
            // The counter can overcount after an interrupted write; reconcile it
            // once before believing the cap is full.
            let Some(tx) = self.claim_cap_slot(tx).await? else {
                if std::mem::take(&mut can_retry_after_reconcile)
                    && reconcile_user_counter_overcount(&self.conn).await?
                {
                    continue;
                }
                return self.reject_over_cap(username, password).await;
            };
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
                    return self.login_after_lost_insert(username, password).await;
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Claim a counter slot in `tx`. The transaction comes back when a slot
    /// was taken; a full cap rolls it back and returns `None`. An uncapped
    /// store always has a slot.
    async fn claim_cap_slot(&self, tx: libsql::Transaction) -> Result<Option<libsql::Transaction>> {
        let MaxUsers::Limited(max) = self.max_users else {
            return Ok(Some(tx));
        };
        if claim_user_counter_slot(&tx, max).await? {
            return Ok(Some(tx));
        }
        tx.rollback().await?;
        Ok(None)
    }

    /// The cap really is full: a caller who already has an account still logs
    /// in, anyone else is refused.
    async fn reject_over_cap(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        if let Some(stored) = self.stored_hash(username).await? {
            return verify_returning_user(username, password, stored).await;
        }
        let MaxUsers::Limited(max) = self.max_users else {
            return Err(RegistryError::RegistrationDisabled);
        };
        Err(RegistryError::TooManyUsers { max })
    }

    /// A concurrent writer took the name first: treat the registration as a
    /// login against whatever they stored.
    async fn login_after_lost_insert(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        let Some(stored) = self.stored_hash(username).await? else {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        };
        verify_returning_user(username, password, stored).await
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
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows = self
                .conn
                .query("SELECT username FROM tokens WHERE token_hash = ?1", params![token_hash])
                .await?;
            match rows.next().await? {
                Some(row) => Ok(Some(row.get::<String>(0)?)),
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
    let cidr_whitelist: Vec<String> =
        serde_json::from_str(&cidr_json).map_err(|err| RegistryError::Internal {
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
        },
    ))
}

#[cfg(test)]
mod tests;
