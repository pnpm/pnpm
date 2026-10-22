pub(super) mod sqlite;

use super::{
    AtomicU64, Connection, Digest, HashMap, HashSet, Mutex, Ordering, PathBuf, RegistryError,
    Result, Sha256, SystemTime, TokenBackend, UNIX_EPOCH, async_trait,
};
use sqlite::{delete_token, init_tokens_schema, insert_token, load_all_tokens};
use std::fmt::Write as _;

/// SHA-256-hashed (`token_hash` → username) map, optionally backed by
/// a `SQLite` database for cross-restart durability.
///
/// Token records carry the verdaccio shape (`created_at`, `last_used_at`,
/// readonly, `cidr_whitelist`) so they can be surfaced by the
/// `/-/npm/v1/tokens` endpoints without a schema migration.
#[derive(Debug)]
pub struct TokenStore {
    pub(super) inner: Mutex<TokenInner>,
    pub(super) persist: Option<PathBuf>,
    pub(super) secret: [u8; 32],
    pub(super) counter: AtomicU64,
}

#[derive(Debug, Default)]
pub(super) struct TokenInner {
    /// hex-encoded SHA-256 of the raw token → record.
    tokens: HashMap<String, TokenRecord>,
    /// username → the keys of its tokens, so work on one user's tokens does
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
            .entry(record.username.clone())
            .or_default()
            .insert(key.clone());
        self.tokens.insert(key, record);
    }

    fn remove(&mut self, key: &str) {
        let Some(record) = self.tokens.remove(key) else {
            return;
        };
        if let Some(keys) = self.by_owner.get_mut(&record.username) {
            keys.remove(key);
            if keys.is_empty() {
                self.by_owner.remove(&record.username);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    fn owned_by(&self, username: &str) -> Vec<(String, TokenRecord)> {
        self.by_owner
            .get(username)
            .into_iter()
            .flatten()
            .filter_map(|key| Some((key.clone(), self.tokens.get(key)?.clone())))
            .collect()
    }
}

#[non_exhaustive]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NewToken {
    pub readonly: bool,
    pub cidr_whitelist: Vec<String>,
    pub name: Option<String>,
    pub expires_at: Option<u64>,
    pub scopes: Option<Vec<String>>,
    pub kind: Option<String>,
}

impl NewToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    #[must_use]
    pub fn with_cidr_whitelist(mut self, cidr_whitelist: Vec<String>) -> Self {
        self.cidr_whitelist = cidr_whitelist;
        self
    }

    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    #[must_use]
    pub fn with_expires_at(mut self, expires_at: Option<u64>) -> Self {
        self.expires_at = expires_at;
        self
    }

    #[must_use]
    pub fn with_scopes(mut self, scopes: Option<Vec<String>>) -> Self {
        self.scopes = scopes;
        self
    }

    #[must_use]
    pub fn with_kind(mut self, kind: Option<String>) -> Self {
        self.kind = kind;
        self
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the auth tokens table columns."
    )
)]
pub struct TokenRecord {
    pub username: String,
    pub created_at: u64,
    pub last_used_at: u64,
    pub readonly: bool,
    pub cidr_whitelist: Vec<String>,
    pub name: Option<String>,
    pub expires_at: Option<u64>,
    pub scopes: Option<Vec<String>>,
    pub kind: Option<String>,
}

impl TokenRecord {
    #[must_use]
    pub fn new(username: impl Into<String>, created_at: u64, last_used_at: u64) -> Self {
        Self {
            username: username.into(),
            created_at,
            last_used_at,
            readonly: false,
            cidr_whitelist: Vec::new(),
            name: None,
            expires_at: None,
            scopes: None,
            kind: None,
        }
    }

    #[must_use]
    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    #[must_use]
    pub fn with_cidr_whitelist(mut self, cidr_whitelist: Vec<String>) -> Self {
        self.cidr_whitelist = cidr_whitelist;
        self
    }

    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    #[must_use]
    pub fn with_expires_at(mut self, expires_at: Option<u64>) -> Self {
        self.expires_at = expires_at;
        self
    }

    #[must_use]
    pub fn with_scopes(mut self, scopes: Option<Vec<String>>) -> Self {
        self.scopes = scopes;
        self
    }

    #[must_use]
    pub fn with_kind(mut self, kind: Option<String>) -> Self {
        self.kind = kind;
        self
    }
}

impl TokenStore {
    /// Pure in-memory store. Tokens vanish on restart.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            inner: Mutex::new(TokenInner::default()),
            persist: None,
            secret: fresh_secret(),
            counter: AtomicU64::new(0),
        }
    }

    /// SQLite-backed store. Creates the file (and the `tokens`
    /// table) if missing; loads existing records into memory on
    /// startup so the hot lookup path doesn't touch disk.
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
        let conn = Connection::open(&path)?;
        init_tokens_schema(&conn)?;
        let tokens = load_all_tokens(&conn)?;
        drop(conn);
        Ok(Self {
            inner: Mutex::new(TokenInner::from_tokens(tokens)),
            persist: Some(path),
            secret: fresh_secret(),
            counter: AtomicU64::new(0),
        })
    }

    async fn persist_issued(&self, token_hash: &str, record: &TokenRecord) -> Result<()> {
        let Some(path) = self.persist.clone() else {
            return Ok(());
        };
        let hash_for_db = token_hash.to_string();
        let record_for_db = record.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(&path)?;
            insert_token(&conn, &hash_for_db, &record_for_db)?;
            Ok(())
        })
        .await;
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
                inner.remove(token_hash);
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
                inner.remove(token_hash);
                Err(err.into())
            }
        }
    }
}

#[async_trait]
impl TokenBackend for TokenStore {
    async fn issue_token(
        &self,
        username: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let raw = mint_token(&self.secret, nonce, username);
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_seconds();
        let record = TokenRecord {
            username: username.to_string(),
            created_at: now,
            last_used_at: now,
            readonly: new_token.readonly,
            cidr_whitelist: new_token.cidr_whitelist,
            name: new_token.name,
            expires_at: new_token.expires_at,
            scopes: new_token.scopes,
            kind: new_token.kind,
        };
        {
            let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
            inner.insert(token_hash.clone(), record.clone());
        }
        self.persist_issued(&token_hash, &record).await?;
        Ok((raw, record))
    }

    async fn issue(&self, username: &str) -> Result<String> {
        let (raw, _) = self.issue_token(username, NewToken::default()).await?;
        Ok(raw)
    }

    /// Resolves entirely in memory — the on-disk mirror is loaded once
    /// at startup, so this never touches the database and never fails.
    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        let now = unix_seconds();
        let Some(record) = inner.tokens.get(&token_hash) else {
            return Ok(None);
        };
        if record.expires_at.is_some_and(|exp| exp <= now) {
            return Ok(None);
        }
        Ok(Some(record.username.clone()))
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        let should_persist = {
            let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
            if let Some(record) = inner.tokens.get_mut(key) {
                let old = record.last_used_at;
                record.last_used_at = now;
                self.persist.is_some() && now != old
            } else {
                false
            }
        };
        if should_persist && let Some(path) = self.persist.clone() {
            let key = key.to_string();
            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = Connection::open(&path)?;
                conn.execute(
                    "UPDATE tokens SET last_used_at = ?1 WHERE token_hash = ?2",
                    rusqlite::params![now as i64, key],
                )?;
                Ok(())
            })
            .await??;
        }
        Ok(())
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        Ok(inner.tokens.get(key).cloned())
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        Ok(inner.owned_by(username))
    }

    /// `SQLite` gets the `DELETE` *before* the in-memory map is mutated.
    /// If the disk write fails, both views still hold the token and
    /// the caller sees a 5xx — the opposite ordering would leave a
    /// "revoked in memory but resurrected on restart" hole.
    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let snapshot = {
            let inner = self.inner.lock().expect("TokenStore mutex poisoned");
            inner.tokens.get(key).cloned()
        };
        let Some(record) = snapshot else {
            return Ok(None);
        };
        if let Some(path) = self.persist.clone() {
            let key = key.to_string();
            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = Connection::open(&path)?;
                delete_token(&conn, &key)?;
                Ok(())
            })
            .await??;
        }
        {
            let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
            inner.remove(key);
        }
        Ok(Some(record))
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

pub(super) fn mint_token(secret: &[u8; 32], nonce: u64, username: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret);
    hasher.update(nonce.to_le_bytes());
    hasher.update(username.as_bytes());
    let digest = hasher.finalize();
    // 16 bytes of hash → 32 hex chars. Long enough to be
    // unguessable, short enough to keep test logs readable.
    hex_encode(&digest[..16])
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

pub(super) fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}
