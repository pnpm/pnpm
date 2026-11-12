pub use import::{ImportMode, PasswordCollisions, PasswordImport};

use super::{
    AccountCredential, AccountStatus, CredentialKind, DEFAULT_BCRYPT_COST, Result, UpsertOutcome,
    account::deprovisioned_login_error, hash_bcrypt, sha256_hex, unix_millis,
    verify_returning_user, write_atomic,
};
use pnpr_config::MaxUsers;
use pnpr_error::RegistryError;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

mod backend;
mod file_stamp;
mod import;
mod stored;

use file_stamp::FileStamp;
use stored::{StoredAccount, Users, parse_accounts};

#[derive(Debug, Default)]
struct UserStoreState {
    users: Users,
    registered_count: u64,
    names_by_id: HashMap<String, String>,
    active_ids: HashSet<String>,
    last_synced: Option<FileStamp>,
    last_digest: Option<String>,
    synced_at: Option<std::time::SystemTime>,
}

impl UserStoreState {
    fn replace_users(&mut self, users: Users) {
        self.registered_count = registered_user_count(&users);
        self.names_by_id = non_deleted_names_by_id(&users);
        self.active_ids = active_user_ids(&users);
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
    tokens: std::sync::OnceLock<std::sync::Weak<crate::TokenStore>>,
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
        let (users, last_synced, last_digest) = match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let parsed = parse_accounts(&raw)
                    .map_err(|reason| RegistryError::InvalidUsersFile {
                        path: path.display().to_string(),
                        reason,
                    })?;
                (parsed, stamp, Some(sha256_hex(raw.as_bytes())))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (HashMap::new(), None, None),
            Err(err) => return Err(err.into()),
        };
        let synced_at = last_synced.as_ref().map(|_| now);
        Ok(Self {
            state: Arc::new(Mutex::new(UserStoreState {
                registered_count: registered_user_count(&users),
                names_by_id: non_deleted_names_by_id(&users),
                active_ids: active_user_ids(&users),
                users,
                last_synced,
                last_digest,
                synced_at,
            })),
            path: Some(path),
            max_users,
            bcrypt_cost,
            edit_lock: Arc::default(),
            tokens: std::sync::OnceLock::new(),
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
                registered_count: registered_user_count(&users),
                names_by_id: non_deleted_names_by_id(&users),
                active_ids: active_user_ids(&users),
                users,
                last_synced: None,
                last_digest: None,
                synced_at: None,
            })),
            path,
            max_users,
            bcrypt_cost,
            edit_lock: Arc::default(),
            tokens: std::sync::OnceLock::new(),
        }
    }

    pub(crate) fn bind_tokens(&self, tokens: &Arc<crate::TokenStore>) {
        let _ = self.tokens.set(Arc::downgrade(tokens));
    }

    pub(super) fn bound_tokens(&self) -> Option<Arc<crate::TokenStore>> {
        self.tokens.get().and_then(std::sync::Weak::upgrade)
    }

    #[cfg(test)]
    pub(crate) fn insert_user_for_test(&self, username: String, hash: String) {
        let mut state = self.snapshot();
        state.users.insert(username, StoredAccount::new_with_hash(hash));
        state.registered_count = registered_user_count(&state.users);
        state.names_by_id = non_deleted_names_by_id(&state.users);
        state.active_ids = active_user_ids(&state.users);
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
                state.last_digest = None;
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
        let digest = sha256_hex(raw.as_bytes());
        if state.last_digest.as_deref() != Some(digest.as_str()) {
            let fresh = parse_accounts(&raw)
                .map_err(|reason| RegistryError::InvalidUsersFile {
                    path: path.display().to_string(),
                    reason,
                })?;
            state.replace_users(fresh);
            state.last_digest = Some(digest);
        }
        state.last_synced = Some(stamp);
        state.synced_at = Some(std::time::SystemTime::now());
        Ok(())
    }

    /// Whether the account ID has `status = active`.
    pub(crate) fn holds_active(&self, user_id: &str) -> Result<bool> {
        self.sync_from_disk_if_path()?;
        Ok(self.snapshot().active_ids.contains(user_id))
    }

    fn snapshot(&self) -> std::sync::MutexGuard<'_, UserStoreState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reject registration before spending time hashing a new password.
    fn check_registration_capacity(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                if self.snapshot().registered_count >= max {
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
            Ok(raw) => {
                let digest = sha256_hex(raw.as_bytes());
                {
                    let guard = state.lock().unwrap_or_else(PoisonError::into_inner);
                    if guard.last_digest.as_deref() == Some(digest.as_str()) {
                        return Ok(guard.users.clone());
                    }
                }
                parse_accounts(&raw)
                    .map_err(|reason| RegistryError::InvalidUsersFile {
                        path: path.display().to_string(),
                        reason,
                    })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(err) => Err(err.into()),
        }
    }
}

fn persist_users_to_disk(
    path: Option<&std::path::Path>,
    users: &Users,
) -> Result<Option<(Option<FileStamp>, String)>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = serde_json::to_vec_pretty(users).map_err(RegistryError::Json)?;
    write_atomic(path, &bytes)?;
    let sync = std::fs::metadata(path).ok().map(|meta| FileStamp::from_metadata(&meta));
    Ok(Some((sync, sha256_hex(&bytes))))
}

impl UserStore {
    /// the copy to the file, then make it the current users.
    ///
    /// The whole edit runs on a blocking thread under the edit lock, so a
    /// cancelled request cannot release the lock while its write is still
    /// going: edits reach the file in the order they reach memory. A failed
    /// write leaves the users as they were, so a repeated edit tries again.
    async fn update_with_post_commit<Out, Edit, Post>(
        &self,
        edit: Edit,
        post_commit: Post,
    ) -> Result<Out>
    where
        Out: Send + 'static,
        Edit: FnOnce(&mut Users) -> Result<Change<Out>> + Send + 'static,
        Post: FnOnce(&Out) -> Result<()> + Send + 'static,
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
            let persisted = persist_users_to_disk(path.as_deref(), &next)?;
            let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
            guard.replace_users(next);
            if let Some((new_sync, digest)) = persisted {
                guard.last_digest = Some(digest);
                if new_sync.is_some() {
                    guard.last_synced = new_sync;
                    guard.synced_at = Some(std::time::SystemTime::now());
                }
            }
            drop(guard);
            post_commit(&out)?;
            Ok(out)
        })
        .await?
    }

    async fn update<Out, Edit>(&self, edit: Edit) -> Result<Out>
    where
        Out: Send + 'static,
        Edit: FnOnce(&mut Users) -> Result<Change<Out>> + Send + 'static,
    {
        self.update_with_post_commit(edit, |_| Ok(())).await
    }

    async fn login_existing(
        &self,
        username: &str,
        password: &str,
        stored: StoredAccount,
    ) -> Result<(UpsertOutcome, String)> {
        if stored.status == AccountStatus::Deactivated {
            return Err(deprovisioned_login_error(username));
        }
        let credential = stored.credential();
        let (outcome, _) =
            verify_returning_user(username, password, &credential, self.bcrypt_cost).await?;
        let user_id = stored.id.clone();
        if stored.credential_kind == CredentialKind::Sha1 {
            let new_hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
            let name = username.to_string();
            self.update(move |users| {
                if let Some(current) = users.get_mut(&name)
                    && current.id == stored.id
                    && current.status == AccountStatus::Active
                    && current.password_hash == stored.password_hash
                {
                    current.set_hash(new_hash);
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

fn registered_user_count(users: &Users) -> u64 {
    users
        .values()
        .filter(|account| !account.status.is_deleted())
        .count() as u64
}

fn non_deleted_names_by_id(users: &Users) -> HashMap<String, String> {
    users
        .iter()
        .filter(|(_, account)| !account.status.is_deleted())
        .map(|(name, account)| (account.id.clone(), name.clone()))
        .collect()
}

fn active_user_ids(users: &Users) -> HashSet<String> {
    users
        .values()
        .filter(|account| account.status.is_active())
        .map(|account| account.id.clone())
        .collect()
}

fn check_username_skeleton(users: &Users, username: &str, own_id: Option<&str>) -> Result<()> {
    let skeleton = crate::validation::username_skeleton_hash(username);
    if users
        .iter()
        .any(|(name, account)| {
            !account.status.is_deleted()
                && Some(account.id.as_str()) != own_id
                && crate::validation::username_skeleton_hash(name) == skeleton
        })
    {
        return Err(crate::validation::confusable_username());
    }
    Ok(())
}
