use super::{
    DEFAULT_BCRYPT_COST, PasswordChange, Result, UpsertOutcome, UserBackend, UserCreation,
    UserRemoval, hash_bcrypt, parse_htpasswd, serialize_htpasswd, validate_username,
    verify_returning_user, write_atomic,
};
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

/// File-backed (or in-memory) htpasswd store.
#[derive(Debug)]
pub struct UserStore {
    pub(crate) users: Arc<Mutex<Users>>,
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
        let users = match std::fs::read_to_string(&path) {
            Ok(raw) => parse_htpasswd(&raw)
                .map_err(|reason| RegistryError::InvalidHtpasswdFile {
                    path: path.display().to_string(),
                    reason,
                })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(err) => return Err(err.into()),
        };
        Ok(Self::with_users(users, Some(path), max_users, bcrypt_cost))
    }

    fn with_users(
        users: Users,
        path: Option<PathBuf>,
        max_users: MaxUsers,
        bcrypt_cost: u32,
    ) -> Self {
        Self {
            users: Arc::new(Mutex::new(users)),
            path,
            max_users,
            bcrypt_cost,
            edit_lock: Arc::default(),
        }
    }

    /// Whether `username` is stored.
    pub(crate) fn holds(&self, username: &str) -> bool {
        self.snapshot().contains_key(username)
    }

    fn snapshot(&self) -> std::sync::MutexGuard<'_, Users> {
        self.users.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reject registration before spending time hashing a new password.
    fn check_registration_capacity(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                if self.snapshot().len() as u64 >= max {
                    return Err(RegistryError::TooManyUsers { max });
                }
            }
            MaxUsers::Unlimited => {}
        }
        Ok(())
    }

    /// Apply `edit` to a copy of the users. When it reports a write, save
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
        let users = Arc::clone(&self.users);
        let edit_lock = Arc::clone(&self.edit_lock);
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let _edit = edit_lock.lock().unwrap_or_else(PoisonError::into_inner);
            let mut next = users
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let out = match edit(&mut next)? {
                Change::Keep(out) => return Ok(out),
                Change::Write(out) => out,
            };
            if let Some(path) = &path {
                write_atomic(path, serialize_htpasswd(&next).as_bytes())?;
            }
            *users.lock().unwrap_or_else(PoisonError::into_inner) = next;
            Ok(out)
        })
        .await?
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

        let existing_hash = self.snapshot().get(username).cloned();
        if let Some(stored) = existing_hash {
            return verify_returning_user(username, password, stored).await;
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
            Registration::Existing(stored) => {
                verify_returning_user(username, password, stored).await
            }
        }
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = self
            .snapshot()
            .keys()
            .cloned()
            .collect();
        names.sort();
        Ok(names)
    }

    async fn password_hash(&self, username: &str) -> Result<Option<String>> {
        Ok(self.snapshot().get(username).cloned())
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
}

impl Default for UserStore {
    fn default() -> Self {
        Self::in_memory()
    }
}
