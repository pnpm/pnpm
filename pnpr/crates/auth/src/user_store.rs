use super::{
    DEFAULT_BCRYPT_COST, PasswordChange, Result, UpsertOutcome, UserBackend, UserCreation,
    UserRemoval, dummy_verify, hash_bcrypt, is_sha_hash, parse_htpasswd, serialize_htpasswd,
    verify_returning_user, write_atomic,
};
use crate::validation::validate_username;
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::RegistryError;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

/// `username -> bcrypt hash`. The hash string carries its own version and
/// cost (`$2y$10$...`) so we never need to remember per-record metadata.
type Users = HashMap<String, String>;

mod file_stamp;

use file_stamp::FileStamp;

#[derive(Debug, Default)]
struct UserStoreState {
    users: Users,
    last_synced: Option<FileStamp>,
    synced_at: Option<std::time::SystemTime>,
}

/// File-backed (or in-memory) htpasswd store.
#[derive(Debug)]
pub struct UserStore {
    state: Arc<Mutex<UserStoreState>>,
    path: Option<PathBuf>,
    max_users: MaxUsers,
    bcrypt_cost: u32,
    /// Held for the whole of an edit: copying the users, writing the copy,
    /// and making it current. See [`UserStore::update`].
    edit_lock: Arc<Mutex<()>>,
}

/// What an edit passed to [`UserStore::update`] did to its copy of the
/// users, and what the edit returns.
enum Change<Out> {
    Keep(Out),
    Write(Out),
}

impl UserStore {
    /// In-memory store with no on-disk persistence and open registration.
    /// Used by registry-mock-compatible programmatic routers.
    #[must_use]
    pub fn in_memory() -> Self {
        Self::in_memory_with_max_users(MaxUsers::Unlimited)
    }

    /// In-memory store that enforces the resolved registration cap.
    #[must_use]
    pub fn in_memory_with_max_users(max_users: MaxUsers) -> Self {
        Self::with_users(Users::new(), None, max_users, DEFAULT_BCRYPT_COST)
    }

    /// In-memory store with a configurable bcrypt cost, for tests that want
    /// sub-100ms hashing.
    #[cfg(test)]
    pub(crate) fn in_memory_with_cost(max_users: MaxUsers, bcrypt_cost: u32) -> Self {
        Self::with_users(Users::new(), None, max_users, bcrypt_cost)
    }

    /// File-backed store. The file is parsed up front so a malformed
    /// htpasswd surfaces as a startup error rather than a silent
    /// empty user list. A missing file is OK — it's created on the
    /// first registration.
    pub fn open(path: PathBuf, max_users: MaxUsers) -> Result<Self> {
        Self::open_with_cost(path, max_users, DEFAULT_BCRYPT_COST)
    }

    /// Like [`Self::open`] but with a configurable bcrypt cost — used
    /// by tests that want sub-100ms hashing.
    pub fn open_with_cost(path: PathBuf, max_users: MaxUsers, bcrypt_cost: u32) -> Result<Self> {
        let now = std::time::SystemTime::now();
        let stamp =
            std::fs::metadata(&path).ok().map(|metadata| FileStamp::from_metadata(&metadata));
        let (users, last_synced) = match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let parsed = parse_htpasswd(&raw)
                    .map_err(|reason| RegistryError::InvalidHtpasswdFile {
                        path: path.display().to_string(),
                        reason,
                    })?;
                (parsed, stamp)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (HashMap::new(), None),
            Err(err) => return Err(err.into()),
        };
        let synced_at = last_synced.as_ref().map(|_| now);
        Ok(Self {
            state: Arc::new(Mutex::new(UserStoreState { users, last_synced, synced_at })),
            path: Some(path),
            max_users,
            bcrypt_cost,
            edit_lock: Arc::default(),
        })
    }

    fn with_users(
        users: Users,
        path: Option<PathBuf>,
        max_users: MaxUsers,
        bcrypt_cost: u32,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(UserStoreState {
                users,
                last_synced: None,
                synced_at: None,
            })),
            path,
            max_users,
            bcrypt_cost,
            edit_lock: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_user_for_test(&self, username: String, hash: String) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .users
            .insert(username, hash);
    }

    #[cfg(test)]
    pub(crate) fn is_empty_for_test(&self) -> bool {
        self.snapshot().users.is_empty()
    }

    fn sync_from_disk_if_path(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if !state.users.is_empty() {
                    state.users.clear();
                }
                state.last_synced = None;
                state.synced_at = None;
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };
        if !meta.is_file() {
            return Ok(());
        }
        let stamp = FileStamp::from_metadata(&meta);
        if let Some(prev) = state.last_synced
            && prev == stamp
            && !stamp.is_racy(state.synced_at)
        {
            return Ok(());
        }
        let raw = std::fs::read_to_string(path)?;
        let fresh = parse_htpasswd(&raw)
            .map_err(|reason| RegistryError::InvalidHtpasswdFile {
                path: path.display().to_string(),
                reason,
            })?;
        state.users = fresh;
        state.last_synced = Some(stamp);
        state.synced_at = Some(std::time::SystemTime::now());
        Ok(())
    }

    /// Whether `username` is stored.
    pub(crate) fn holds(&self, username: &str) -> Result<bool> {
        self.sync_from_disk_if_path()?;
        Ok(self.snapshot().users.contains_key(username))
    }

    fn snapshot(&self) -> std::sync::MutexGuard<'_, UserStoreState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reject registration before spending time hashing a new password.
    fn check_registration_capacity(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                if self.snapshot().users.len() as u64 >= max {
                    return Err(RegistryError::TooManyUsers { max });
                }
            }
            MaxUsers::Unlimited => {}
        }
        Ok(())
    }

    fn load_users_for_update(
        path: Option<&std::path::Path>,
        state: &Arc<Mutex<UserStoreState>>,
    ) -> Result<Users> {
        let Some(path) = path else {
            return Ok(state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .users
                .clone());
        };
        match std::fs::read_to_string(path) {
            Ok(raw) => parse_htpasswd(&raw)
                .map_err(|reason| RegistryError::InvalidHtpasswdFile {
                    path: path.display().to_string(),
                    reason,
                }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(err) => Err(err.into()),
        }
    }
}

fn lock_htpasswd(path: Option<&std::path::Path>) -> Result<Option<std::fs::File>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("htpasswd.lock"))?;
    lock_file.lock()?;
    Ok(Some(lock_file))
}

fn persist_users_to_disk(
    path: Option<&std::path::Path>,
    users: &Users,
) -> Result<Option<FileStamp>> {
    let Some(path) = path else {
        return Ok(None);
    };
    write_atomic(path, serialize_htpasswd(users).as_bytes())?;
    let sync = std::fs::metadata(path).ok().map(|meta| FileStamp::from_metadata(&meta));
    Ok(sync)
}

impl UserStore {
    /// the copy to the file, then make it the current users.
    ///
    /// The whole edit runs on a blocking thread under the edit lock, so a
    /// cancelled request cannot release the lock while its write is still
    /// going: edits reach the file in the order they reach memory. A failed
    /// write leaves the users as they were, so a repeated edit tries again.
    async fn update<Out, Edit>(&self, edit: Edit) -> Result<Out>
    where
        Out: Send + 'static,
        Edit: FnOnce(&mut Users) -> Result<Change<Out>> + Send + 'static,
    {
        let state = Arc::clone(&self.state);
        let edit_lock = Arc::clone(&self.edit_lock);
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let _edit = edit_lock.lock().unwrap_or_else(PoisonError::into_inner);
            let _file_lock = lock_htpasswd(path.as_deref())?;
            let mut next = Self::load_users_for_update(path.as_deref(), &state)?;
            let out = match edit(&mut next)? {
                Change::Keep(out) => return Ok(out),
                Change::Write(out) => out,
            };
            let new_sync = persist_users_to_disk(path.as_deref(), &next)?;
            let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
            guard.users = next;
            if new_sync.is_some() {
                guard.last_synced = new_sync;
                guard.synced_at = Some(std::time::SystemTime::now());
            }
            Ok(out)
        })
        .await?
    }

    async fn login_existing(
        &self,
        username: &str,
        password: &str,
        stored: String,
    ) -> Result<(UpsertOutcome, String)> {
        let (outcome, user) =
            verify_returning_user(username, password, stored.clone(), self.bcrypt_cost).await?;
        if is_sha_hash(&stored) {
            let new_hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
            let name = username.to_string();
            self.update(move |users| {
                if users.get(&name) == Some(&stored) {
                    users.insert(name, new_hash);
                    Ok(Change::Write(()))
                } else {
                    Ok(Change::Keep(()))
                }
            })
            .await?;
        }
        Ok((outcome, user))
    }
}

/// The outcome of a registration's locked step.
enum Registration {
    Created,
    /// Another request registered the name while this one hashed.
    Existing(String),
}

#[async_trait]
impl UserBackend for UserStore {
    /// * Unknown username, registration allowed → bcrypt the password,
    ///   insert, persist, return `Created`.
    /// * Known username, password matches → return `LoggedIn`.
    /// * Known username, password wrong → `Unauthenticated`.
    /// * Unknown username, registration disabled or capped →
    ///   `RegistrationDisabled` / `TooManyUsers`.
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;
        self.sync_from_disk_if_path()?;

        let existing_hash = self
            .snapshot()
            .users
            .get(username)
            .cloned();
        if let Some(stored) = existing_hash {
            return self.login_existing(username, password, stored).await;
        }

        if matches!(self.max_users, MaxUsers::Disabled) {
            dummy_verify(password, self.bcrypt_cost).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }

        self.check_registration_capacity()?;

        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        let max_users = self.max_users;
        let registration = self.update(move |users| {
            if let Some(stored) = users.get(&name) {
                return Ok(Change::Keep(Registration::Existing(stored.clone())));
            }
            // Re-check under the lock because another registration may
            // have filled the store while we were hashing.
            if let MaxUsers::Limited(max) = max_users
                && users.len() as u64 >= max
            {
                return Err(RegistryError::TooManyUsers { max });
            }
            users.insert(name, hash);
            Ok(Change::Write(Registration::Created))
        });
        match registration.await? {
            Registration::Created => Ok((UpsertOutcome::Created, username.to_string())),
            Registration::Existing(stored) => self.login_existing(username, password, stored).await,
        }
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        self.sync_from_disk_if_path()?;
        let mut names: Vec<String> = self
            .snapshot()
            .users
            .keys()
            .cloned()
            .collect();
        names.sort();
        Ok(names)
    }

    async fn password_hash(&self, username: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .cloned())
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        validate_username(username)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        self.update(move |users| {
            if users.contains_key(&name) {
                return Ok(Change::Keep(UserCreation::NameTaken));
            }
            users.insert(name, hash);
            Ok(Change::Write(UserCreation::Created))
        })
        .await
    }

    async fn set_password(&self, username: &str, password: &str) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        self.update(move |users| match users.get_mut(&name) {
            Some(stored) => {
                *stored = hash;
                Ok(Change::Write(PasswordChange::Changed))
            }
            None => Ok(Change::Keep(PasswordChange::NoSuchUser)),
        })
        .await
    }

    async fn delete_user(&self, username: &str) -> Result<UserRemoval> {
        let name = username.to_string();
        self.update(move |users| match users.remove(&name) {
            Some(_) => Ok(Change::Write(UserRemoval::Removed)),
            None => Ok(Change::Keep(UserRemoval::NoSuchUser)),
        })
        .await
    }

    async fn check_readiness(&self) -> Result<()> {
        if let Some(path) = &self.path {
            if path.exists() {
                tokio::fs::metadata(path).await.map_err(RegistryError::Io)?;
            } else if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                tokio::fs::create_dir_all(parent).await.map_err(RegistryError::Io)?;
            }
        }
        Ok(())
    }
}

impl Default for UserStore {
    fn default() -> Self {
        Self::in_memory()
    }
}
