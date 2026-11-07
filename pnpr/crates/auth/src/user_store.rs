pub use import::{ImportMode, PasswordCollisions, PasswordImport};

use super::{
    DEFAULT_BCRYPT_COST, Result, UpsertOutcome, hash_bcrypt, is_sha_hash, verify_returning_user,
    write_atomic,
};
use pnpr_config::MaxUsers;
use pnpr_error::RegistryError;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

/// Account records indexed by their current login name.
type Users = HashMap<String, StoredAccount>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAccount {
    id: String,
    password_hash: String,
}

impl StoredAccount {
    fn new(password_hash: String) -> Self {
        Self { id: crate::new_principal_id(), password_hash }
    }
}

fn parse_accounts(raw: &str) -> std::result::Result<Users, String> {
    let accounts: Users = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let mut ids = std::collections::HashSet::new();
    for (name, account) in &accounts {
        if account.password_hash != crate::DELETED_USER_HASH {
            crate::validation::validate_username(name).map_err(|err| err.to_string())?;
        } else if name != &format!("deleted:{}", account.id) {
            return Err("deleted accounts must use their generated names".to_string());
        }
        if account.id.is_empty() || !ids.insert(account.id.as_str()) {
            return Err("account IDs must be nonempty and unique".to_string());
        }
        if account.password_hash != crate::DELETED_USER_HASH
            && account.password_hash != crate::NO_PASSWORD_HASH
            && !crate::htpasswd::is_supported_hash(&account.password_hash)
        {
            return Err(format!("unsupported password hash for {name:?}"));
        }
    }
    Ok(accounts)
}

mod backend;
mod file_stamp;
mod import;

use file_stamp::FileStamp;

#[derive(Debug, Default)]
struct UserStoreState {
    users: Users,
    active_count: u64,
    names_by_id: HashMap<String, String>,
    last_synced: Option<FileStamp>,
    synced_at: Option<std::time::SystemTime>,
}

impl UserStoreState {
    fn replace_users(&mut self, users: Users) {
        self.active_count = active_user_count(&users);
        self.names_by_id = active_names_by_id(&users);
        self.users = users;
    }
}

/// Native JSON account store, with an in-memory variant.
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
    /// account file surfaces as a startup error rather than a silent
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
                let parsed = parse_accounts(&raw)
                    .map_err(|reason| RegistryError::InvalidUsersFile {
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
            state: Arc::new(Mutex::new(UserStoreState {
                active_count: active_user_count(&users),
                names_by_id: active_names_by_id(&users),
                users,
                last_synced,
                synced_at,
            })),
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
                active_count: active_user_count(&users),
                names_by_id: active_names_by_id(&users),
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
        let mut state = self.snapshot();
        state.users.insert(username, StoredAccount::new(hash));
        state.active_count = active_user_count(&state.users);
        state.names_by_id = active_names_by_id(&state.users);
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
                state.replace_users(Users::new());
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
        let fresh = parse_accounts(&raw)
            .map_err(|reason| RegistryError::InvalidUsersFile {
                path: path.display().to_string(),
                reason,
            })?;
        state.replace_users(fresh);
        state.last_synced = Some(stamp);
        state.synced_at = Some(std::time::SystemTime::now());
        Ok(())
    }

    /// Whether the account ID is active.
    pub(crate) fn holds(&self, user_id: &str) -> Result<bool> {
        self.sync_from_disk_if_path()?;
        Ok(self.snapshot().names_by_id.contains_key(user_id))
    }

    fn snapshot(&self) -> std::sync::MutexGuard<'_, UserStoreState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reject registration before spending time hashing a new password.
    fn check_registration_capacity(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                if self.snapshot().active_count >= max {
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
            Ok(raw) => parse_accounts(&raw)
                .map_err(|reason| RegistryError::InvalidUsersFile {
                    path: path.display().to_string(),
                    reason,
                }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(err) => Err(err.into()),
        }
    }
}

fn persist_users_to_disk(
    path: Option<&std::path::Path>,
    users: &Users,
) -> Result<Option<FileStamp>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = serde_json::to_vec_pretty(users).map_err(RegistryError::Json)?;
    write_atomic(path, &bytes)?;
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
            let _file_lock = path
                .as_deref()
                .map(crate::lock_htpasswd)
                .transpose()?;
            let mut next = Self::load_users_for_update(path.as_deref(), &state)?;
            let out = match edit(&mut next)? {
                Change::Keep(out) => return Ok(out),
                Change::Write(out) => out,
            };
            let new_sync = persist_users_to_disk(path.as_deref(), &next)?;
            let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
            guard.replace_users(next);
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
        stored: StoredAccount,
    ) -> Result<(UpsertOutcome, String)> {
        let (outcome, _) = verify_returning_user(
            username,
            password,
            stored.password_hash.clone(),
            self.bcrypt_cost,
        )
        .await?;
        let user_id = stored.id.clone();
        if is_sha_hash(&stored.password_hash) {
            let new_hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
            let name = username.to_string();
            self.update(move |users| {
                if let Some(current) = users.get_mut(&name)
                    && current.id == stored.id
                    && current.password_hash == stored.password_hash
                {
                    current.password_hash = new_hash;
                    Ok(Change::Write(()))
                } else {
                    Ok(Change::Keep(()))
                }
            })
            .await?;
        }
        Ok((outcome, user_id))
    }
}

/// The outcome of a registration's locked step.
enum Registration {
    Created(String),
    /// Another request registered the name while this one hashed.
    Existing(StoredAccount),
}

impl Default for UserStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

fn active_user_count(users: &Users) -> u64 {
    users
        .values()
        .filter(|account| account.password_hash != super::DELETED_USER_HASH)
        .count() as u64
}

fn active_names_by_id(users: &Users) -> HashMap<String, String> {
    users
        .iter()
        .filter(|(_, account)| account.password_hash != crate::DELETED_USER_HASH)
        .map(|(name, account)| (account.id.clone(), name.clone()))
        .collect()
}
