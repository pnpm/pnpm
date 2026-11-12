pub(crate) mod format;
pub(super) mod sqlite;

pub(crate) use format::mint_token;
pub use format::{PERSONAL_TOKEN_LEN, PERSONAL_TOKEN_PREFIX, has_valid_personal_token_checksum};
pub(super) use pnpm_crypto_hash::create_hex_hash_bytes as sha256_hex;
pub use record::{NewToken, TokenKind, TokenRecord};

mod record;

use super::{
    Arc, Connection, HashMap, HashSet, Mutex, Path, PathBuf, RegistryError, Result, SystemTime,
    TokenBackend, UNIX_EPOCH, async_trait, validation::token_timestamp_to_sql,
};
use sqlite::{
    TokenDatabase, delete_expired_tokens, delete_token, delete_user_tokens, insert_token,
};

/// SHA-256-hashed (`token_hash` → `user_id`) map, optionally backed by
/// a `SQLite` database for cross-restart durability.
///
/// Every operation that reads the map first reloads it if another process,
/// such as `pnpr add-user`, committed to the database since the last load.
///
/// Token records carry the verdaccio shape (`created_at`, `last_used_at`,
/// readonly, `cidr_whitelist`) so they can be surfaced by the
/// `/-/npm/v1/tokens` endpoints without a schema migration.
#[derive(Debug)]
pub struct TokenStore {
    pub(super) inner: Arc<Mutex<TokenInner>>,
    pub(super) persist: Option<PathBuf>,
    /// Serializes writes, and holds the database connection when persisted.
    database: Arc<Mutex<Option<TokenDatabase>>>,
}

#[derive(Debug, Default)]
pub(super) struct TokenInner {
    /// hex-encoded SHA-256 of the raw token → record.
    tokens: HashMap<String, TokenRecord>,
    /// `user_id` → the keys of its tokens, so work on one user's tokens does
    /// not scan everyone's while holding the lock lookups need.
    by_owner: HashMap<String, HashSet<String>>,
}

impl TokenInner {
    fn from_tokens(tokens: HashMap<String, TokenRecord>) -> Self {
        let mut inner = Self::default();
        for (key, record) in tokens {
            inner.insert(key, record);
        }
        inner
    }

    pub(super) fn insert(&mut self, key: String, record: TokenRecord) {
        self.by_owner
            .entry(record.user_id.clone())
            .or_default()
            .insert(key.clone());
        self.tokens.insert(key, record);
    }

    fn remove(&mut self, key: &str) -> Option<TokenRecord> {
        let record = self.tokens.remove(key)?;
        if let Some(keys) = self.by_owner.get_mut(&record.user_id) {
            keys.remove(key);
            if keys.is_empty() {
                self.by_owner.remove(&record.user_id);
            }
        }
        Some(record)
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    fn owned_by(&self, user_id: &str) -> Vec<(String, TokenRecord)> {
        self.by_owner
            .get(user_id)
            .into_iter()
            .flatten()
            .filter_map(|key| Some((key.clone(), self.tokens.get(key)?.clone())))
            .collect()
    }

    fn remove_expired(&mut self, now: u64) -> usize {
        let expired_keys: Vec<String> = self.tokens
            .iter()
            .filter(|(_, record)| record.expires_at.is_some_and(|exp| exp <= now))
            .map(|(key, _)| key.clone())
            .collect();
        let removed = expired_keys.len();
        for key in &expired_keys {
            self.remove(key);
        }
        removed
    }

    fn remove_for_user(&mut self, user_id: &str) -> usize {
        let Some(keys) = self.by_owner.remove(user_id) else {
            return 0;
        };
        let mut removed = 0;
        for key in keys {
            if self.tokens.remove(&key).is_some() {
                removed += 1;
            }
        }
        removed
    }
}

impl TokenStore {
    /// Pure in-memory store. Tokens vanish on restart.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TokenInner::default())),
            persist: None,
            database: Arc::new(Mutex::new(None)),
        }
    }

    /// SQLite-backed store. Creates the file (and the `tokens`
    /// table) if missing, and loads existing records into memory.
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            if !path.exists() {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path);
            }
        }
        let (database, tokens) = TokenDatabase::open(&path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(TokenInner::from_tokens(tokens))),
            persist: Some(path),
            database: Arc::new(Mutex::new(Some(database))),
        })
    }

    async fn store_issued(&self, token_hash: String, record: TokenRecord) -> Result<()> {
        if self.persist.is_none() {
            return store_issued_sync(&self.database, &self.inner, token_hash, record);
        }
        let inner = Arc::clone(&self.inner);
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || {
            store_issued_sync(&database, &inner, token_hash, record)
        })
        .await?
    }

    /// The in-memory map, reloaded first if another connection changed the
    /// database.
    fn synced_inner(&self) -> Result<std::sync::MutexGuard<'_, TokenInner>> {
        let mut database = lock_database(&self.database);
        sync_from_database(&mut database, &self.inner)?;
        Ok(self.inner.lock().expect("TokenStore mutex poisoned"))
    }
}

#[async_trait]
impl TokenBackend for TokenStore {
    async fn issue_token(
        &self,
        user_id: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let raw = mint_token();
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_millis();
        let record = TokenRecord {
            owner_name: None,
            user_id: user_id.to_string(),
            created_at: now,
            last_used_at: now,
            readonly: new_token.readonly,
            cidr_whitelist: new_token.cidr_whitelist,
            name: new_token.name,
            expires_at: new_token.expires_at,
            kind: new_token.kind,
        };
        self.store_issued(token_hash, record.clone()).await?;
        Ok((raw, record))
    }

    async fn issue(&self, user_id: &str) -> Result<String> {
        let (raw, _) = self.issue_token(user_id, NewToken::default()).await?;
        Ok(raw)
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let inner = self.synced_inner()?;
        let now = unix_millis();
        let Some(record) = inner.tokens.get(&token_hash) else {
            return Ok(None);
        };
        if record.expires_at.is_some_and(|exp| exp <= now) {
            return Ok(None);
        }
        Ok(Some(record.user_id.clone()))
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        let needs_touch = self.inner
            .lock()
            .expect("TokenStore mutex poisoned")
            .tokens
            .get(key)
            .is_some_and(|record| record.last_used_at != now);
        if !needs_touch {
            return Ok(());
        }
        if self.persist.is_none() {
            return touch_sync(&self.database, &self.inner, key, now);
        }
        let key = key.to_string();
        let inner = Arc::clone(&self.inner);
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || touch_sync(&database, &inner, &key, now)).await?
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let inner = self.synced_inner()?;
        Ok(inner.tokens.get(key).cloned())
    }

    async fn list_for_user(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(self.synced_inner()?.owned_by(user_id))
    }

    /// `SQLite` gets the `DELETE` *before* the in-memory map is mutated.
    /// If the disk write fails, both views still hold the token and
    /// the caller sees a 5xx — the opposite ordering would leave a
    /// "revoked in memory but resurrected on restart" hole.
    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        if self.persist.is_none() {
            return revoke_key_sync(&self.database, &self.inner, key);
        }
        let key = key.to_string();
        let inner = Arc::clone(&self.inner);
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || revoke_key_sync(&database, &inner, &key)).await?
    }

    async fn revoke_all_for_user(&self, user_id: &str) -> Result<usize> {
        if self.persist.is_none() {
            return revoke_user_sync(&self.database, &self.inner, user_id);
        }
        let owner = user_id.to_string();
        let inner = Arc::clone(&self.inner);
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || revoke_user_sync(&database, &inner, &owner)).await?
    }

    async fn delete_expired(&self, now: u64) -> Result<usize> {
        if self.persist.is_none() {
            return delete_expired_sync(&self.database, &self.inner, now);
        }
        let inner = Arc::clone(&self.inner);
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || delete_expired_sync(&database, &inner, now)).await?
    }

    async fn check_readiness(&self) -> Result<()> {
        let Some(path) = self.persist.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || -> Result<()> {
            let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE;
            let conn = Connection::open_with_flags(&path, flags)?;
            conn.query_row("SELECT EXISTS(SELECT 1 FROM tokens LIMIT 1);", [], |_row| Ok(()))?;
            Ok(())
        })
        .await??;
        Ok(())
    }
}

impl TokenStore {
    pub(crate) fn revoke_all_for_user_blocking(&self, user_id: &str) -> Result<usize> {
        revoke_user_sync(&self.database, &self.inner, user_id)
    }

    #[cfg(test)]
    pub(crate) fn lock_database_for_test(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<TokenDatabase>> {
        lock_database(&self.database)
    }
}

fn lock_database(
    database: &Mutex<Option<TokenDatabase>>,
) -> std::sync::MutexGuard<'_, Option<TokenDatabase>> {
    database.lock().expect("TokenStore database mutex poisoned")
}

fn sync_from_database(
    database: &mut Option<TokenDatabase>,
    inner: &Mutex<TokenInner>,
) -> Result<()> {
    let Some(database) = database else {
        return Ok(());
    };
    if let Some(tokens) = database.load_if_changed()? {
        *inner.lock().expect("TokenStore mutex poisoned") = TokenInner::from_tokens(tokens);
    }
    Ok(())
}

fn store_issued_sync(
    database: &Mutex<Option<TokenDatabase>>,
    inner: &Mutex<TokenInner>,
    token_hash: String,
    record: TokenRecord,
) -> Result<()> {
    let mut database = lock_database(database);
    sync_from_database(&mut database, inner)?;
    if let Some(database) = database.as_ref() {
        insert_token(database.connection(), &token_hash, &record)?;
    }
    inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .insert(token_hash, record);
    Ok(())
}

fn touch_sync(
    database: &Mutex<Option<TokenDatabase>>,
    inner: &Mutex<TokenInner>,
    key: &str,
    now: u64,
) -> Result<()> {
    let mut database = lock_database(database);
    sync_from_database(&mut database, inner)?;
    let should_update = inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .tokens
        .get(key)
        .is_some_and(|record| record.last_used_at != now);
    if !should_update {
        return Ok(());
    }
    if let Some(database) = database.as_ref() {
        database
            .connection()
            .execute(
                "UPDATE tokens SET last_used_at = ?1 WHERE token_hash = ?2",
                rusqlite::params![token_timestamp_to_sql(now), key],
            )?;
    }
    if let Some(record) = inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .tokens
        .get_mut(key)
    {
        record.last_used_at = now;
    }
    Ok(())
}

fn revoke_key_sync(
    database: &Mutex<Option<TokenDatabase>>,
    inner: &Mutex<TokenInner>,
    key: &str,
) -> Result<Option<TokenRecord>> {
    let mut database = lock_database(database);
    sync_from_database(&mut database, inner)?;
    let snapshot = inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .tokens
        .get(key)
        .cloned();
    if snapshot.is_none() {
        return Ok(None);
    }
    if let Some(database) = database.as_ref() {
        delete_token(database.connection(), key)?;
    }
    Ok(inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .remove(key))
}

fn revoke_user_sync(
    database: &Mutex<Option<TokenDatabase>>,
    inner: &Mutex<TokenInner>,
    user_id: &str,
) -> Result<usize> {
    let mut database = lock_database(database);
    sync_from_database(&mut database, inner)?;
    let db_removed = if let Some(database) = database.as_ref() {
        delete_user_tokens(database.connection(), user_id)?
    } else {
        0
    };
    let mem_removed = inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .remove_for_user(user_id);
    Ok(mem_removed.max(db_removed))
}

fn delete_expired_sync(
    database: &Mutex<Option<TokenDatabase>>,
    inner: &Mutex<TokenInner>,
    now: u64,
) -> Result<usize> {
    let mut database = lock_database(database);
    sync_from_database(&mut database, inner)?;
    let db_removed = if let Some(database) = database.as_ref() {
        delete_expired_tokens(database.connection(), now)?
    } else {
        0
    };
    let mem_removed = inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .remove_expired(now);
    Ok(mem_removed.max(db_removed))
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

// ---------------------------------------------------------------
// crypto helpers
// ---------------------------------------------------------------

/// Build a freshly-randomized secret for [`TokenStore::issue`].
/// Pulls 32 bytes from the OS CSPRNG (`getrandom` → `/dev/urandom`
/// on Linux, `BCryptGenRandom` on Windows, `getentropy` on macOS).
/// We refuse to start the server if the OS RNG is unavailable
/// rather than fall back to weaker entropy — token unguessability
/// is the whole reason this exists.
pub(super) fn fresh_secret() -> [u8; 32] {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).expect("OS CSPRNG must be available");
    secret
}

/// Current application time in milliseconds since the Unix epoch.
#[must_use]
pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}
