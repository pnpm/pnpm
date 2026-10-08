use super::{
    DEFAULT_BCRYPT_COST, Result, UpsertOutcome, UserBackend, hash_bcrypt, parse_htpasswd,
    serialize_htpasswd, validate_username, verify_returning_user, write_atomic,
};
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::RegistryError;
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

/// File-backed (or in-memory) htpasswd store.
#[derive(Debug)]
pub struct UserStore {
    /// `username -> bcrypt hash`. The hash string carries its own
    /// version and cost (`$2y$10$...`) so we never need to remember
    /// per-record metadata.
    pub(crate) users: Mutex<HashMap<String, String>>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) max_users: MaxUsers,
    pub(crate) bcrypt_cost: u32,
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
        Self {
            users: Mutex::new(HashMap::new()),
            path: None,
            max_users,
            bcrypt_cost: DEFAULT_BCRYPT_COST,
        }
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
        Ok(Self { users: Mutex::new(users), path: Some(path), max_users, bcrypt_cost })
    }

    /// Reject registration before spending time hashing a new password.
    fn check_registration_capacity(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                let current = self.users
                    .lock()
                    .expect("UserStore mutex poisoned")
                    .len() as u64;
                if current >= max {
                    return Err(RegistryError::TooManyUsers { max });
                }
            }
            MaxUsers::Unlimited => {}
        }
        Ok(())
    }

    /// Apply `edit` to the users under the lock and persist the result
    /// when `edit` reports a change.
    async fn update(
        &self,
        edit: impl FnOnce(&mut HashMap<String, String>) -> bool,
    ) -> Result<bool> {
        let snapshot = {
            let mut users = self.users.lock().expect("UserStore mutex poisoned");
            if !edit(&mut users) {
                return Ok(false);
            }
            serialize_htpasswd(&users)
        };
        self.persist(snapshot).await?;
        Ok(true)
    }

    async fn persist(&self, body: String) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || write_atomic(&path, body.as_bytes())).await??;
        Ok(())
    }
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

        let existing_hash = {
            let users = self.users.lock().expect("UserStore mutex poisoned");
            users.get(username).cloned()
        };
        if let Some(stored) = existing_hash {
            return verify_returning_user(username, password, stored).await;
        }

        self.check_registration_capacity()?;

        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        enum NextStep {
            Persist(String),
            VerifyExisting(String),
        }
        let next_step = {
            let mut users = self.users.lock().expect("UserStore mutex poisoned");
            match (users.get(username).cloned(), self.max_users) {
                (Some(stored), _) => NextStep::VerifyExisting(stored),
                // Re-check under the lock because another registration may
                // have filled the store while we were hashing.
                (None, MaxUsers::Limited(max)) if users.len() as u64 >= max => {
                    return Err(RegistryError::TooManyUsers { max });
                }
                (None, _) => {
                    users.insert(username.to_string(), hash);
                    NextStep::Persist(serialize_htpasswd(&users))
                }
            }
        };
        match next_step {
            NextStep::Persist(snapshot) => {
                self.persist(snapshot).await?;
                Ok((UpsertOutcome::Created, username.to_string()))
            }
            NextStep::VerifyExisting(stored) => {
                verify_returning_user(username, password, stored).await
            }
        }
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = self.users
            .lock()
            .expect("UserStore mutex poisoned")
            .keys()
            .cloned()
            .collect();
        names.sort();
        Ok(names)
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<bool> {
        validate_username(username)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        self.update(|users| {
            if users.contains_key(username) {
                return false;
            }
            users.insert(username.to_string(), hash);
            true
        })
        .await
    }

    async fn set_password(&self, username: &str, password: &str) -> Result<bool> {
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        self.update(|users| match users.get_mut(username) {
            Some(stored) => {
                *stored = hash;
                true
            }
            None => false,
        })
        .await
    }

    async fn delete_user(&self, username: &str) -> Result<bool> {
        self.update(|users| users.remove(username).is_some()).await
    }
}

impl Default for UserStore {
    fn default() -> Self {
        Self::in_memory()
    }
}
