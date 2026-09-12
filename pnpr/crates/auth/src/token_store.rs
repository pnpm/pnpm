use super::{
    AtomicU64, Connection, Digest, HashMap, Mutex, Ordering, PathBuf, RegistryError, Result,
    Sha256, SystemTime, TokenBackend, UNIX_EPOCH, async_trait, token_timestamp_from_sql,
    token_timestamp_to_sql,
};
use std::fmt::Write as _;

/// SHA-256-hashed (`token_hash` → username) map, optionally backed by
/// a `SQLite` database for cross-restart durability.
///
/// Token records carry the verdaccio shape (`created_at`, `last_used_at`,
/// readonly, `cidr_whitelist`) so they can be surfaced by future
/// `/-/npm/v1/tokens` endpoints without a schema migration.
#[derive(Debug)]
pub struct TokenStore {
    pub(super) inner: Mutex<TokenInner>,
    pub(super) persist: Option<PathBuf>,
    pub(super) secret: [u8; 32],
    pub(super) counter: AtomicU64,
}

#[derive(Debug)]
pub(super) struct TokenInner {
    /// hex-encoded SHA-256 of the raw token → record.
    pub(super) tokens: HashMap<String, TokenRecord>,
}

#[derive(Debug, Clone)]
pub struct TokenRecord {
    pub username: String,
    pub created_at: u64,
    pub last_used_at: u64,
    pub readonly: bool,
    pub cidr_whitelist: Vec<String>,
}

impl TokenStore {
    /// Pure in-memory store. Tokens vanish on restart.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            inner: Mutex::new(TokenInner { tokens: HashMap::new() }),
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
        let conn = Connection::open(&path)?;
        init_tokens_schema(&conn)?;
        let tokens = load_all_tokens(&conn)?;
        drop(conn);
        Ok(Self {
            inner: Mutex::new(TokenInner { tokens }),
            persist: Some(path),
            secret: fresh_secret(),
            counter: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl TokenBackend for TokenStore {
    async fn issue(&self, username: &str) -> Result<String> {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let raw = mint_token(&self.secret, nonce, username);
        let token_hash = sha256_hex(raw.as_bytes());
        let record = TokenRecord {
            username: username.to_string(),
            created_at: unix_seconds(),
            last_used_at: unix_seconds(),
            readonly: false,
            cidr_whitelist: Vec::new(),
        };
        {
            let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
            inner.tokens.insert(token_hash.clone(), record.clone());
        }
        if let Some(path) = self.persist.clone() {
            let hash_for_db = token_hash.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = Connection::open(&path)?;
                insert_token(&conn, &hash_for_db, &record)?;
                Ok(())
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
                    inner.tokens.remove(&token_hash);
                    return Err(err);
                }
                Err(err) => {
                    let mut inner = self.inner.lock().expect("TokenStore mutex poisoned");
                    inner.tokens.remove(&token_hash);
                    return Err(err.into());
                }
            }
        }
        Ok(raw)
    }

    /// Resolves entirely in memory — the on-disk mirror is loaded once
    /// at startup, so this never touches the database and never fails.
    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        Ok(inner.tokens.get(&token_hash).map(|record| record.username.clone()))
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        Ok(inner.tokens.get(key).cloned())
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        Ok(inner
            .tokens
            .iter()
            .filter(|(_, record)| record.username == username)
            .map(|(hash, record)| (hash.clone(), record.clone()))
            .collect())
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
            inner.tokens.remove(key);
        }
        Ok(Some(record))
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

// ---------------------------------------------------------------
// SQLite-backed token store
// ---------------------------------------------------------------

/// `tokens` table DDL — shared by every SQL-backed auth store so the
/// backends store the same shape and records can be moved between them.
pub(super) const TOKENS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS tokens (
    token_hash      CHAR(64) PRIMARY KEY,
    username        VARCHAR(255) NOT NULL,
    created_at      BIGINT NOT NULL,
    last_used_at    BIGINT NOT NULL,
    readonly        SMALLINT NOT NULL DEFAULT 0,
    cidr_whitelist  VARCHAR(4096) NOT NULL DEFAULT '[]'
)";

pub(super) const TOKENS_INDEX_SQL: &str =
    "CREATE INDEX IF NOT EXISTS tokens_username ON tokens(username)";

pub(super) fn init_tokens_schema(conn: &Connection) -> Result<()> {
    conn.execute(TOKENS_TABLE_SQL, [])?;
    conn.execute(TOKENS_INDEX_SQL, [])?;
    Ok(())
}

pub(super) fn load_all_tokens(conn: &Connection) -> Result<HashMap<String, TokenRecord>> {
    let mut stmt = conn.prepare(
        "SELECT token_hash, username, created_at, last_used_at, readonly, cidr_whitelist
         FROM tokens",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = HashMap::new();
    while let Some(row) = rows.next()? {
        let hash: String = row.get(0)?;
        let username: String = row.get(1)?;
        let created_at: i64 = row.get(2)?;
        let last_used_at: i64 = row.get(3)?;
        let readonly: i64 = row.get(4)?;
        let cidr_json: String = row.get(5)?;
        let cidr_whitelist: Vec<String> =
            serde_json::from_str(&cidr_json).map_err(|err| RegistryError::Internal {
                reason: format!("token {hash} has an unreadable cidr_whitelist: {err}"),
            })?;
        out.insert(
            hash,
            TokenRecord {
                username,
                created_at: token_timestamp_from_sql(created_at),
                last_used_at: token_timestamp_from_sql(last_used_at),
                readonly: readonly != 0,
                cidr_whitelist,
            },
        );
    }
    Ok(out)
}

pub(super) fn delete_token(conn: &Connection, token_hash: &str) -> Result<()> {
    conn.execute("DELETE FROM tokens WHERE token_hash = ?1", rusqlite::params![token_hash])?;
    Ok(())
}

pub(super) fn insert_token(
    conn: &Connection,
    token_hash: &str,
    record: &TokenRecord,
) -> Result<()> {
    let cidr_json = serde_json::to_string(&record.cidr_whitelist)
        .expect("Vec<String> always serializes to JSON");
    conn.execute(
        "INSERT INTO tokens (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            token_hash,
            record.username,
            token_timestamp_to_sql(record.created_at),
            token_timestamp_to_sql(record.last_used_at),
            i64::from(record.readonly),
            cidr_json,
        ],
    )?;
    Ok(())
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
