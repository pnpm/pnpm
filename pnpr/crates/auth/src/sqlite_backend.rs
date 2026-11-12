//! Accounts and tokens in a `SQLite` database: the metadata database of a
//! single-instance registry, or a private in-memory database.
//!
//! Every write takes the database's write lock (`BEGIN IMMEDIATE`), so a
//! check and the write it decides commit together. Account names are
//! checked inside the transaction instead of relying on the unique indexes
//! to refuse a duplicate, and the registration cap counts the `users` rows
//! directly. The `auth_counters` row the shared databases keep is not read
//! here: a shared backend reconciles it with the `users` rows at startup.

pub use import::{ImportMode, ImportedPasswords, PasswordCollisions, PasswordImport};

mod account_reads;
mod accounts;
mod import;
mod tokens;

use crate::{
    AccountCredential, AccountLifecycle, AccountLogin, AccountStatus, AccountStatusOutcome,
    AccountStatusRevision, DEFAULT_BCRYPT_COST, PasswordChange, UpsertOutcome, UserAccount,
    UserBackend, UserCreation, UserRemoval, UserRename,
};
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::{RegistryError, Result};
use pnpr_sql::{
    adapter::{Adapter as _, Connection, Param, ReadSource, WriteTransaction},
    sqlite::SqliteAdapter,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Account and token store in one `SQLite` database.
pub struct SqliteAuth {
    database: Arc<SqliteAdapter>,
    /// The database file, or `None` for an in-memory database.
    path: Option<PathBuf>,
    max_users: MaxUsers,
    bcrypt_cost: u32,
}

impl std::fmt::Debug for SqliteAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("SqliteAuth").finish_non_exhaustive()
    }
}

impl SqliteAuth {
    /// Open the database at `path`, creating it and migrating its schema if
    /// needed.
    pub fn open(path: &Path, max_users: MaxUsers) -> Result<Self> {
        restrict_to_owner(path)?;
        let auth = Self::with_database(SqliteAdapter::open(path)?, max_users)?;
        Ok(Self { path: Some(path.to_path_buf()), ..auth })
    }

    /// A private in-memory copy of the accounts in the database at `path`,
    /// empty when there is none. A preview runs on it, so it neither creates
    /// nor migrates the database.
    pub fn preview_of(path: &Path, max_users: MaxUsers) -> Result<Self> {
        let auth = Self::in_memory(max_users)?;
        if !path.exists() {
            return Ok(auth);
        }
        let accounts = read_account_rows(path)?;
        auth.database.write(&mut |connection| {
            for row in &accounts {
                let params: Vec<Param<'_>> = row.iter().map(to_param).collect::<Result<_>>()?;
                connection.execute(
                    &format!("INSERT INTO users ({ACCOUNT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"),
                    &params,
                )?;
            }
            Ok(())
        })?;
        Ok(auth)
    }

    /// A private database that lives in memory, for programmatic routers and
    /// tests.
    pub fn in_memory(max_users: MaxUsers) -> Result<Self> {
        Self::with_database(SqliteAdapter::in_memory()?, max_users)
    }

    fn with_database(database: SqliteAdapter, max_users: MaxUsers) -> Result<Self> {
        database.initialize(&mut |connection| {
            pnpr_sql::schema::ensure(connection, "SQLite auth database")
        })?;
        Ok(Self {
            database: Arc::new(database),
            path: None,
            max_users,
            bcrypt_cost: DEFAULT_BCRYPT_COST,
        })
    }

    /// Hash new passwords at `cost` instead of [`DEFAULT_BCRYPT_COST`], for
    /// tests that need fast hashing.
    #[must_use]
    pub fn with_bcrypt_cost(mut self, cost: u32) -> Self {
        self.bcrypt_cost = cost;
        self
    }

    /// Run `operation` on a read connection on the blocking pool.
    async fn read<Output, Operation>(&self, operation: Operation) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnOnce(&dyn Connection) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || {
            let mut operation = Some(operation);
            let mut output = None;
            database.read(ReadSource::Primary, &mut |connection| {
                let operation = operation.take().expect("an SQLite read runs once");
                output = Some(operation(connection)?);
                Ok(())
            })?;
            Ok(output.expect("a successful SQLite read sets its output"))
        })
        .await?
    }

    /// Run `operation` in a write transaction on the blocking pool, committing
    /// what it wrote when it succeeds.
    async fn write<Output, Operation>(&self, operation: Operation) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnOnce(&dyn Connection) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || {
            let mut operation = Some(operation);
            let mut output = None;
            database.write(&mut |connection| {
                let operation = operation.take().expect("an SQLite write runs once");
                let transaction = WriteTransaction::begin(connection)?;
                let written = operation(&*transaction)?;
                transaction.commit()?;
                output = Some(written);
                Ok(())
            })?;
            Ok(output.expect("a successful SQLite write sets its output"))
        })
        .await?
    }
}

/// Every column of `users`, in the order [`SqliteAuth::preview_of`] copies them.
const ACCOUNT_COLUMNS: &str = "id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at, name_revision, status_revision";

fn read_account_rows(path: &Path) -> Result<Vec<Vec<rusqlite::types::Value>>> {
    let source = open_unchanged(path)?;
    let mut statement = source.prepare(&format!("SELECT {ACCOUNT_COLUMNS} FROM users"))?;
    let rows = statement
        .query_map([], |row| {
            (0..9)
                .map(|index| row.get(index))
                .collect()
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Open `path` for reading without creating files beside it. Without a
/// `-wal` file every commit is in the database file, which `SQLite` then
/// reads as immutable instead of creating its `-wal` and `-shm` files. With
/// one, those files exist already and hold commits a reader must see.
fn open_unchanged(path: &Path) -> Result<rusqlite::Connection> {
    use rusqlite::OpenFlags;
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        return Ok(rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?);
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let uri = immutable_uri(&std::path::absolute(path)?);
    Ok(rusqlite::Connection::open_with_flags(uri, flags)?)
}

/// A `file:` URI that opens the absolute `path` as an immutable database.
fn immutable_uri(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from(if path.starts_with('/') { "file://" } else { "file:///" });
    for character in path.chars() {
        match character {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3F"),
            '#' => uri.push_str("%23"),
            other => uri.push(other),
        }
    }
    uri.push_str("?immutable=1");
    uri
}

fn to_param(value: &rusqlite::types::Value) -> Result<Param<'_>> {
    use rusqlite::types::Value;
    match value {
        Value::Null => Ok(Param::Null),
        Value::Integer(integer) => Ok(Param::Integer(*integer)),
        Value::Text(text) => Ok(Param::Text(text)),
        Value::Blob(blob) => Ok(Param::Blob(blob)),
        Value::Real(_) => Err(RegistryError::Internal {
            reason: "an account column holds a floating-point value".to_string(),
        }),
    }
}

/// Give the database file, and the `-wal` and `-shm` files `SQLite` keeps
/// beside it, owner-only permissions: they hold password and token hashes.
/// `SQLite` creates its sidecars with the database file's permissions.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err.into()),
    }
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_os_string();
        file.push(suffix);
        match std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<()> {
    Ok(())
}

#[async_trait]
impl UserBackend for SqliteAuth {
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        self.add_or_login_account(username, password).await
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        Ok(self
            .list_accounts()
            .await?
            .into_iter()
            .map(|account| account.username)
            .collect())
    }

    async fn list_accounts(&self) -> Result<Vec<UserAccount>> {
        self.read(account_reads::list_accounts).await
    }

    async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<UserAccount>> {
        let ids = serde_json::to_string(user_ids)?;
        self.read(move |connection| account_reads::accounts_by_id(connection, &ids)).await
    }

    async fn credential(&self, username: &str) -> Result<Option<AccountCredential>> {
        Ok(self
            .stored_account(username)
            .await?
            .filter(|account| account.status.is_active())
            .map(|account| account.credential))
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        self.create_account(username, password).await
    }

    async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange> {
        crate::validation::validate_password(password)?;
        let hash = crate::hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        self.replace_password_hash(user_id, Some(hash)).await
    }

    async fn set_password_if_current(
        &self,
        user_id: &str,
        password: &str,
        expected_hash: &str,
    ) -> Result<bool> {
        crate::validation::validate_password(password)?;
        let hash = crate::hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        self.replace_password_hash_if_current(user_id, expected_hash, hash).await
    }

    async fn clear_password(&self, user_id: &str) -> Result<PasswordChange> {
        self.replace_password_hash(user_id, None).await
    }

    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval> {
        let user_id = user_id.to_string();
        self.write(move |connection| accounts::delete_account(connection, &user_id)).await
    }

    async fn provision_user(&self, username: &str) -> Result<String> {
        self.provision_account(username).await
    }

    async fn account_id(&self, username: &str) -> Result<Option<String>> {
        Ok(self
            .stored_account(username)
            .await?
            .filter(|account| !account.status.is_deleted())
            .map(|account| account.id))
    }

    async fn active_account_id(&self, username: &str) -> Result<Option<String>> {
        Ok(self
            .stored_account(username)
            .await?
            .filter(|account| account.status.is_active())
            .map(|account| account.id))
    }

    async fn account_name(&self, user_id: &str) -> Result<Option<String>> {
        Ok(self.account_login(user_id).await?.map(|login| login.username))
    }

    async fn active_account_name(&self, user_id: &str) -> Result<Option<String>> {
        let user_id = user_id.to_string();
        self.read(move |connection| account_reads::active_account_name(connection, &user_id)).await
    }

    async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>> {
        let user_id = user_id.to_string();
        self.read(move |connection| account_reads::account_status(connection, &user_id)).await
    }

    async fn status_revisions(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountStatusRevision>> {
        let ids = serde_json::to_string(user_ids)?;
        self.read(move |connection| account_reads::status_revisions(connection, &ids)).await
    }

    async fn apply_account_status(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: AccountLifecycle,
    ) -> Result<AccountStatusOutcome> {
        let sql_revision = crate::validation::status_revision_to_sql(revision)?;
        let user_id = user_id.to_string();
        self.write(move |connection| {
            accounts::apply_lifecycle(
                connection,
                &user_id,
                accounts::Lifecycle { revision, sql_revision, target: lifecycle },
            )
        })
        .await
    }

    fn revokes_tokens_in_status_transition(&self) -> bool {
        true
    }

    async fn account_login(&self, user_id: &str) -> Result<Option<AccountLogin>> {
        let user_id = user_id.to_string();
        self.read(move |connection| account_reads::account_login(connection, &user_id)).await
    }

    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
        crate::validation::validate_username(username)?;
        let (user_id, username) = (user_id.to_string(), username.to_string());
        self.write(move |connection| {
            let login =
                account_reads::account_login(connection, &user_id)?.ok_or(RegistryError::NotFound)?;
            let next = crate::validation::name_revision_bounds(login.revision)?
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("user {user_id} has no rename revision left"),
                })?;
            accounts::rename_account(connection, &user_id, &username, next.1)
        })
        .await
    }

    async fn rename_user_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<UserRename> {
        crate::validation::validate_username(username)?;
        let Some((expected, next, next_revision)) =
            crate::validation::name_revision_bounds(expected_revision)?
        else {
            self.account_name(user_id).await?.ok_or(RegistryError::NotFound)?;
            return Ok(UserRename::CurrentChanged);
        };
        let (user_id, username) = (user_id.to_string(), username.to_string());
        self.write(move |connection| {
            let login =
                account_reads::account_login(connection, &user_id)?.ok_or(RegistryError::NotFound)?;
            if i64::try_from(login.revision).ok() != Some(expected) {
                return Ok(UserRename::CurrentChanged);
            }
            accounts::rename_account(connection, &user_id, &username, next)?;
            Ok(UserRename::Renamed(next_revision))
        })
        .await
    }

    /// Open the database file anew, without creating it: the pooled
    /// connections keep working on a file that was deleted or replaced.
    async fn check_readiness(&self) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || {
            let connection = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM users), EXISTS(SELECT 1 FROM tokens)",
                [],
                |_| Ok(()),
            )?;
            Ok(())
        })
        .await?
    }
}
