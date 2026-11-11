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
//! The SQL is identical to the local backend — the `tokens` and `users` tables
//! come from [`pnpr_sql::schema::TABLES`] — so a database can be migrated
//! between the two. Users, which the local backend keeps in a JSON account
//! file, live in the `users` table here.

mod accounts;
mod registration;
mod schema;
mod tokens;
mod user_admin;
use pnpr_libsql::{ConnectionPool, LibsqlConnector, first_row, is_unique_violation};
use schema::{
    claim_user_counter_slot, init_schema, missing_count_row, reconcile_user_counter_overcount,
    retry_database_conflicts,
};

use super::{
    AccountCredential, AccountStatus, CredentialKind, DEFAULT_BCRYPT_COST, PasswordChange,
    TokenBackend, TokenRecord, UpsertOutcome, UserBackend, UserCreation, UserRemoval,
    account::{decode_sql_account_state, deprovisioned_login_error},
    dummy_verify, hash_bcrypt, is_sha_hash, unix_millis,
    validation::{token_timestamp_to_sql, validate_username},
    verify_returning_user, with_auth_timeout,
};
use async_trait::async_trait;
use libsql::{Connection, Database, Row, TransactionBehavior, params};
use pnpr_config::{LibsqlSettings, MaxUsers};
use pnpr_error::{RegistryError, Result};
use std::time::Duration;

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
    max_users: MaxUsers,
    registration_lock: tokio::sync::Mutex<()>,
    /// Whether the pool's database is an embedded replica, which
    /// [`Self::sync_replica`] then brings up to date after a write.
    is_replica: bool,
}

pub(super) struct StoredLibsqlUser {
    pub(super) id: String,
    pub(super) status: AccountStatus,
    pub(super) credential: AccountCredential,
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
    /// With `replicaPath` set, the database is an embedded replica (see
    /// [`pnpr_libsql::open`]), so reads hit the local file. Without it, every
    /// read is a remote query against the primary — always fresh, but a
    /// network round-trip on the auth hot path.
    ///
    /// The whole startup, the initial sync included, runs under one
    /// [`DEFAULT_STARTUP_TIMEOUT`], within which a dial that times out is
    /// retried.
    pub async fn connect(settings: &LibsqlSettings, max_users: MaxUsers) -> Result<Self> {
        let connector = pnpr_libsql::connector(settings, "database")?;
        let _startup_dials = connector.retry_timed_out_dials();
        with_auth_timeout(
            DEFAULT_STARTUP_TIMEOUT,
            Box::pin(Self::open(settings, max_users, connector)),
        )
        .await
        .map_err(|error| pnpr_libsql::startup_error(settings, error))
    }

    async fn open(
        settings: &LibsqlSettings,
        max_users: MaxUsers,
        connector: LibsqlConnector,
    ) -> Result<Self> {
        let db = pnpr_libsql::open(settings, connector).await?;
        let max_connections =
            settings.max_connections.unwrap_or(LibsqlSettings::DEFAULT_MAX_CONNECTIONS);
        let is_replica = settings.replica_path.is_some();
        Self::with_replica(db, max_users, max_connections, is_replica).await
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
        let pool = ConnectionPool::new(db, max_connections as usize, DEFAULT_AUTH_TIMEOUT, || {
            RegistryError::AuthDatabaseTimeout
        });
        let auth = Self {
            pool: if is_replica { pool.replica() } else { pool },
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

    /// The account row stored for `username`, or `None` when no such user exists.
    pub(super) async fn stored_account(&self, username: &str) -> Result<Option<StoredLibsqlUser>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT id, status, credential_kind, password_hash FROM users WHERE username = ?1",
                params![username],
            )
            .await?;
            let Some((id, raw_status, raw_kind, password_hash)) = first_row(rows, |row| {
                Ok((
                    row.get::<String>(0)?,
                    row.get::<String>(1)?,
                    row.get::<String>(2)?,
                    row.get::<Option<String>>(3)?,
                ))
            })
            .await?
            else {
                return Ok(None);
            };
            let (status, credential) =
                decode_sql_account_state(&id, &raw_status, &raw_kind, password_hash)?;
            Ok(Some(StoredLibsqlUser { id, status, credential }))
        })
        .await
    }

    /// Current number of registered users — read under the
    /// registration cap, never on the hot path.
    async fn user_count(&self) -> Result<u64> {
        self.pool.read(|conn| async move {
            let rows =
                conn.query("SELECT COUNT(*) FROM users WHERE status <> 'deleted'", ()).await?;
            let Some(count) = first_row(rows, |row| Ok(row.get::<i64>(0)?)).await? else {
                return Err(missing_count_row());
            };
            Ok(count.max(0) as u64)
        })
        .await
    }

    async fn check_db_readiness(&self) -> Result<()> {
        self.pool.read(|conn| async move {
            first_row(conn.query("SELECT 1;", ()).await?, |_| Ok(())).await?;
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

    async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
        self.list_accounts_impl().await
    }

    async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<crate::UserAccount>> {
        self.accounts_by_id_impl(user_ids).await
    }

    async fn credential(&self, username: &str) -> Result<Option<AccountCredential>> {
        Ok(self
            .stored_account(username)
            .await?
            .filter(|account| account.status.is_active())
            .map(|account| account.credential))
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        let creation = self.insert_user_for_admin(username, password).await?;
        self.sync_replica().await?;
        if creation == UserCreation::NameTaken && self.account_id_by_name(username).await?.is_none()
        {
            return Err(crate::validation::confusable_username());
        }
        Ok(creation)
    }

    async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange> {
        self.update_password(user_id, password).await
    }

    async fn set_password_if_current(
        &self,
        user_id: &str,
        password: &str,
        expected_hash: &str,
    ) -> Result<bool> {
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        self.replace_password_hash_if_current(user_id, expected_hash, &hash).await
    }

    async fn clear_password(&self, user_id: &str) -> Result<PasswordChange> {
        self.replace_password_hash(user_id, None).await
    }

    async fn delete_user(&self, username: &str) -> Result<UserRemoval> {
        let removal = self.remove_user_record(username).await?;
        if removal == UserRemoval::Removed {
            self.sync_replica().await?;
        }
        Ok(removal)
    }

    async fn provision_user(&self, username: &str) -> Result<String> {
        self.provision_account(username).await
    }

    async fn account_id(&self, username: &str) -> Result<Option<String>> {
        self.account_id_by_name(username).await
    }

    async fn active_account_id(&self, username: &str) -> Result<Option<String>> {
        self.active_account_id_by_name(username).await
    }

    async fn account_name(&self, user_id: &str) -> Result<Option<String>> {
        self.account_name_by_id(user_id).await
    }

    async fn active_account_name(&self, user_id: &str) -> Result<Option<String>> {
        self.active_account_name_by_id(user_id).await
    }

    async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>> {
        self.account_status_by_id(user_id).await
    }

    async fn set_account_status(&self, user_id: &str, status: AccountStatus) -> Result<bool> {
        self.set_account_status_by_id(user_id, status).await
    }

    async fn account_login(&self, user_id: &str) -> Result<Option<crate::AccountLogin>> {
        self.account_login_by_id(user_id).await
    }

    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
        self.rename_account(user_id, username).await
    }

    async fn rename_user_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename> {
        self.rename_account_if_current(user_id, expected_revision, username).await
    }

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
    }
}

#[cfg(test)]
mod tests;
