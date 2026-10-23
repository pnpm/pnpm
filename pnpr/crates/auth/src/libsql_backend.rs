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
//! [`super::token_store::sqlite::TOKENS_TABLE_SQL`] verbatim — so a database can be migrated
//! between the two. Users, which the local backend keeps in an htpasswd
//! file, live in a `users` table here ([`super::USERS_TABLE_SQL`]).

mod connector;
mod pool;
mod schema;
mod tokens;
mod user_admin;
use connector::{LibsqlConnector, is_plain_http};
use pool::ConnectionPool;
use schema::{
    claim_user_counter_slot, init_schema, is_unique_violation, missing_count_row,
    reconcile_user_counter_overcount, retry_database_conflicts,
};

use super::{
    DEFAULT_BCRYPT_COST, PasswordChange, TokenBackend, TokenRecord, UpsertOutcome, UserBackend,
    UserCreation, UserRemoval, dummy_verify, fresh_secret, hash_bcrypt, is_sha_hash,
    verify_returning_user, with_auth_timeout,
};
use crate::validation::validate_username;
use async_trait::async_trait;
use libsql::{
    Builder, Connection, Database, Error as LibsqlError, Row, TransactionBehavior, params,
};
use pnpm_network::is_url_secure_for_credentials;
use pnpr_config::{LibsqlSettings, MaxUsers};
use pnpr_error::{RegistryError, Result};
use std::{path::Path, sync::atomic::AtomicU64, time::Duration};

/// Deadline for each auth database operation, beyond which a stalled endpoint
/// surfaces [`RegistryError::AuthDatabaseTimeout`] rather than hanging.
const DEFAULT_AUTH_TIMEOUT: Duration = Duration::from_secs(30);

/// Deadline for the one-time startup connect and schema setup; more
/// generous than [`DEFAULT_AUTH_TIMEOUT`] for cold remote DDL.
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_mins(5);

/// Networked-SQLite auth backend. Both record stores run their operations
/// on one [`ConnectionPool`].
pub struct LibsqlAuth {
    pool: ConnectionPool,
    /// Per-process secret seeding [`crate::token_store::mint_token`]. Need not match other
    /// replicas: tokens are looked up by their stored hash, never
    /// re-derived, so each replica minting with its own secret is safe.
    secret: [u8; 32],
    counter: AtomicU64,
    max_users: MaxUsers,
    registration_lock: tokio::sync::Mutex<()>,
    /// Whether the pool's database is an embedded replica, which
    /// [`Self::sync_replica`] then brings up to date after a write.
    is_replica: bool,
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
    /// file. libsql probes the primary for its newer sync protocol, the
    /// only one Turso Cloud serves, and falls back to the replication
    /// protocol of a self-hosted `sqld`. Opening a connection over the
    /// newer one blocks in place, so the caller must run on tokio's
    /// multi-thread runtime.
    ///
    /// The whole startup, the initial sync included, runs under one
    /// [`DEFAULT_STARTUP_TIMEOUT`], within which a dial that times out is
    /// retried. Without `replicaPath`, every
    /// read is a remote query against the primary — always fresh, but a
    /// network round-trip on the auth hot path.
    pub async fn connect(settings: &LibsqlSettings, max_users: MaxUsers) -> Result<Self> {
        if settings.auth_token.is_some()
            && is_plain_http(&settings.url)
            && !is_url_secure_for_credentials(&settings.url)
        {
            return Err(RegistryError::InvalidConfig {
                reason: "backend.libsql.authToken requires an https:// or libsql:// url, or \
                         http:// on a loopback host"
                    .to_string(),
            });
        }
        let connector = LibsqlConnector::for_url(&settings.url, settings.auth_token.is_some())?;
        let _startup_dials = connector.retry_timed_out_dials();
        with_auth_timeout(
            DEFAULT_STARTUP_TIMEOUT,
            Box::pin(Self::open(settings, max_users, connector)),
        )
        .await
    }

    async fn open(
        settings: &LibsqlSettings,
        max_users: MaxUsers,
        connector: LibsqlConnector,
    ) -> Result<Self> {
        let db = if let Some(path) = &settings.replica_path {
            Self::open_replica(path, settings, connector).await?
        } else {
            let auth_token = settings.auth_token.clone().unwrap_or_default();
            Box::pin(
                Builder::new_remote(settings.url.clone(), auth_token).connector(connector).build(),
            )
            .await?
        };
        let max_connections =
            settings.max_connections.unwrap_or(LibsqlSettings::DEFAULT_MAX_CONNECTIONS);
        let is_replica = settings.replica_path.is_some();
        Self::with_replica(db, max_users, max_connections, is_replica).await
    }

    /// Open the embedded replica at `path`, creating its directory the way
    /// the local stores create theirs.
    async fn open_replica(
        path: &Path,
        settings: &LibsqlSettings,
        connector: LibsqlConnector,
    ) -> Result<Database> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await.map_err(RegistryError::Io)?;
        }
        let auth_token = settings.auth_token.clone().unwrap_or_default();
        // Writes are synced back by `sync_replica` instead.
        let mut builder = Builder::new_remote_replica(path, settings.url.clone(), auth_token)
            .connector(connector)
            .read_your_writes(false);
        let interval =
            settings.sync_interval_secs.unwrap_or(LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS);
        if interval > 0 {
            builder = builder.sync_interval(Duration::from_secs(interval));
        }
        let db = Box::pin(builder.build()).await?;
        if interval == 0 {
            // libsql bootstraps the replica during the build only alongside a
            // sync interval; otherwise it would do so inside the blocking
            // `Database::connect`, which the startup deadline cannot cancel.
            Box::pin(db.sync()).await?;
        }
        Ok(db)
    }

    /// [`Self::with_replica`] for the local-database test setup.
    #[cfg(test)]
    async fn from_database(
        db: Database,
        max_users: MaxUsers,
        max_connections: u32,
    ) -> Result<Self> {
        Self::with_replica(db, max_users, max_connections, false).await
    }

    /// Build the backend from an already-open [`Database`], an embedded
    /// replica when `is_replica` is set, and ensure the `users` and
    /// `tokens` tables exist.
    async fn with_replica(
        db: Database,
        max_users: MaxUsers,
        max_connections: u32,
        is_replica: bool,
    ) -> Result<Self> {
        let auth = Self {
            pool: ConnectionPool::new(db, max_connections as usize, DEFAULT_AUTH_TIMEOUT),
            secret: fresh_secret(),
            counter: AtomicU64::new(0),
            max_users,
            registration_lock: tokio::sync::Mutex::new(()),
            is_replica,
        };
        with_auth_timeout(
            DEFAULT_STARTUP_TIMEOUT,
            init_schema(&auth.pool.connect()?, || auth.sync_replica()),
        )
        .await?;
        Ok(auth)
    }

    /// The bcrypt hash stored for `username`, or `None` when no such
    /// user exists.
    async fn stored_hash(&self, username: &str) -> Result<Option<String>> {
        self.pool.read(|conn| async move {
            let mut rows =
                conn.query("SELECT bcrypt_hash FROM users WHERE username = ?1", params![username])
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
        self.pool.read(|conn| async move {
            let mut rows = conn.query("SELECT COUNT(*) FROM users", ()).await?;
            let Some(row) = rows.next().await? else {
                return Err(missing_count_row());
            };
            let count: i64 = row.get(0)?;
            Ok(count.max(0) as u64)
        })
        .await
    }

    async fn check_db_readiness(&self) -> Result<()> {
        self.pool.read(|conn| async move {
            conn.query("SELECT 1;", ()).await?;
            Ok(())
        })
        .await
    }

    /// Pull the primary's latest frames into the replica, so a write this
    /// backend just delegated is visible to its own reads. libsql's
    /// read-your-writes would pull on the connection that wrote and reset
    /// its pager afterwards, which corrupts a read in flight on it;
    /// [`Database::sync`] pulls on a connection of its own instead.
    async fn sync_replica(&self) -> Result<()> {
        if !self.is_replica {
            return Ok(());
        }
        self.pool.sync().await
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
        let outcome =
            retry_database_conflicts(|| self.add_or_login_attempt(username, password, &hash))
                .await?;
        if matches!(outcome.0, UpsertOutcome::Created) {
            self.sync_replica().await?;
        }
        Ok(outcome)
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        self.list_usernames().await
    }

    async fn password_hash(&self, username: &str) -> Result<Option<String>> {
        self.stored_hash(username).await
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        let creation = self.insert_user_for_admin(username, password).await?;
        if creation == UserCreation::Created {
            self.sync_replica().await?;
        }
        Ok(creation)
    }

    async fn set_password(&self, username: &str, password: &str) -> Result<PasswordChange> {
        let change = self.update_password(username, password).await?;
        if change == PasswordChange::Changed {
            self.sync_replica().await?;
        }
        Ok(change)
    }

    async fn delete_user(&self, username: &str) -> Result<UserRemoval> {
        let removal = self.remove_user(username).await?;
        if removal == UserRemoval::Removed {
            self.sync_replica().await?;
        }
        Ok(removal)
    }

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
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
            return self.verify_and_maybe_upgrade(username, password, stored).await;
        }

        if matches!(self.max_users, MaxUsers::Disabled) {
            dummy_verify(password, DEFAULT_BCRYPT_COST).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
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
        let inserted = self.pool.write(|conn| async move {
            let inserted = conn.execute(
                "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                params![username, hash],
            )
            .await;
            match inserted {
                Ok(_) => Ok(Insert::Created),
                Err(err) if is_unique_violation(&err) => Ok(Insert::NameTaken),
                Err(err) => Err(RegistryError::from(err)),
            }
        })
        .await?;
        match inserted {
            Insert::Created => Ok((UpsertOutcome::Created, username.to_string())),
            Insert::NameTaken => self.login_after_lost_insert(username, password).await,
            Insert::CapFull => unreachable!("an uncapped insert has no cap"),
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
            match self.try_capped_insert(username, hash).await? {
                Insert::Created => return Ok((UpsertOutcome::Created, username.to_string())),
                Insert::NameTaken => return self.login_after_lost_insert(username, password).await,
                // The counter can overcount after an interrupted write; reconcile it
                // once before believing the cap is full.
                Insert::CapFull => {
                    if !std::mem::take(&mut can_retry_after_reconcile)
                        || !self.reconcile_user_counter().await?
                    {
                        return self.reject_over_cap(username, password).await;
                    }
                }
            }
        }
    }

    /// One transaction that claims a counter slot and inserts the user.
    async fn try_capped_insert(&self, username: &str, hash: &str) -> Result<Insert> {
        self.pool.write(|conn| async move {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).await?;
            let Some(tx) = self.claim_cap_slot(tx).await? else {
                return Ok(Insert::CapFull);
            };
            let inserted = tx.execute(
                "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                params![username, hash],
            )
            .await;
            match inserted {
                Ok(_) => {
                    tx.commit().await?;
                    Ok(Insert::Created)
                }
                Err(err) if is_unique_violation(&err) => {
                    tx.rollback().await?;
                    Ok(Insert::NameTaken)
                }
                Err(err) => Err(RegistryError::from(err)),
            }
        })
        .await
    }

    /// Correct a counter that overcounts, reporting whether it did.
    async fn reconcile_user_counter(&self) -> Result<bool> {
        self.pool.write(|conn| async move { reconcile_user_counter_overcount(&conn).await }).await
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
            return self.verify_and_maybe_upgrade(username, password, stored).await;
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
        self.sync_replica().await?;
        let Some(stored) = self.stored_hash(username).await? else {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        };
        self.verify_and_maybe_upgrade(username, password, stored).await
    }

    async fn verify_and_maybe_upgrade(
        &self,
        username: &str,
        password: &str,
        stored: String,
    ) -> Result<(UpsertOutcome, String)> {
        let (outcome, user) =
            verify_returning_user(username, password, stored.clone(), DEFAULT_BCRYPT_COST).await?;
        if is_sha_hash(&stored) {
            let new_hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
            self.pool.write(|conn| async move {
                conn.execute(
                    "UPDATE users SET bcrypt_hash = ?1 WHERE username = ?2 AND bcrypt_hash = ?3",
                    params![new_hash, username, stored],
                )
                .await?;
                Ok(())
            })
            .await?;
            self.sync_replica().await?;
        }
        Ok((outcome, user))
    }
}

/// How a registration's insert ended, decided on its connection and acted on
/// after the connection is returned.
enum Insert {
    Created,
    NameTaken,
    CapFull,
}

#[cfg(test)]
mod tests;
