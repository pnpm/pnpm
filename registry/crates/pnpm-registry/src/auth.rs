//! User and token storage for the registry.
//!
//! Two stores back the auth flow:
//!
//! * [`UserStore`] — username → bcrypt-hashed password. Persisted as
//!   an Apache-style htpasswd file when [`UserStore::open`] is given
//!   a path; in-memory otherwise. The on-disk format is one
//!   `<username>:<bcrypt-hash>` line per user, so the same file can
//!   be inspected and verified by Apache's `htpasswd -v`.
//! * [`TokenStore`] — SHA-256 token hash → token record. Persisted in
//!   a SQLite database when [`TokenStore::open`] is given a path;
//!   in-memory otherwise. The raw token is only returned to the
//!   caller once on `issue`; only its hash ever hits disk so a leak
//!   of the database doesn't grant access on its own.
//!
//! Both stores keep a full mirror of their state in a `Mutex<...>`
//! and persist on every write. Reads (the hot path for
//! `enforce_access`) never touch disk.

use crate::{
    config::{AuthConfig, MaxUsers},
    error::{RegistryError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// Bundle of the user store and the token store. Built once at
/// startup so the rest of the server doesn't have to know whether
/// auth is file-backed or in-memory.
#[derive(Debug)]
pub struct AuthState {
    pub users: UserStore,
    pub tokens: TokenStore,
}

impl AuthState {
    /// All-in-memory auth state. Used when neither
    /// `auth.htpasswd.file` nor `auth.tokens.file` are configured,
    /// and by tests that don't care about persistence.
    pub fn in_memory() -> Self {
        Self { users: UserStore::in_memory(), tokens: TokenStore::in_memory() }
    }

    /// Build the auth state from an [`AuthConfig`]. Either store is
    /// in-memory when its file path is unset; otherwise the on-disk
    /// state is loaded eagerly so a malformed htpasswd or a
    /// permission-denied SQLite file surfaces as a startup error.
    pub fn load(config: &AuthConfig) -> Result<Self> {
        let users = match config.htpasswd.file.clone() {
            Some(path) => UserStore::open(path, config.htpasswd.max_users)?,
            None => UserStore::in_memory(),
        };
        let tokens = match config.tokens.file.clone() {
            Some(path) => TokenStore::open(path)?,
            None => TokenStore::in_memory(),
        };
        Ok(Self { users, tokens })
    }
}

/// Bcrypt cost factor used for new password hashes. Cost 10 is what
/// verdaccio uses by default and matches Apache `htpasswd -B`'s
/// default, so files written here verify cleanly against either
/// tool. ~50–100 ms per hash on modern hardware — slow enough to
/// frustrate offline cracking, cheap enough that adduser doesn't
/// feel sluggish.
const DEFAULT_BCRYPT_COST: u32 = 10;

/// File-backed (or in-memory) htpasswd store.
#[derive(Debug)]
pub struct UserStore {
    /// `username -> bcrypt hash`. The hash string carries its own
    /// version and cost (`$2y$10$...`) so we never need to remember
    /// per-record metadata.
    users: Mutex<HashMap<String, String>>,
    path: Option<PathBuf>,
    max_users: MaxUsers,
    bcrypt_cost: u32,
}

impl UserStore {
    /// In-memory store with no on-disk persistence. Used when
    /// `auth.htpasswd.file` is unset and by the existing
    /// `@pnpm/registry-mock` integration where every restart is a
    /// fresh process.
    pub fn in_memory() -> Self {
        Self {
            users: Mutex::new(HashMap::new()),
            path: None,
            max_users: MaxUsers::Unlimited,
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
            Ok(raw) => parse_htpasswd(&raw).map_err(|reason| {
                RegistryError::InvalidHtpasswdFile { path: path.display().to_string(), reason }
            })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { users: Mutex::new(users), path: Some(path), max_users, bcrypt_cost })
    }

    /// Add a new user or verify a returning one.
    ///
    /// * Unknown username, registration allowed → bcrypt the password,
    ///   insert, persist, return `Created`.
    /// * Known username, password matches → return `LoggedIn`.
    /// * Known username, password wrong → `Unauthenticated`.
    /// * Unknown username, registration disabled or capped →
    ///   `RegistrationDisabled` / `TooManyUsers`.
    pub async fn add_or_login(&self, username: &str, password: &str) -> Result<UpsertOutcome> {
        let existing_hash = {
            let users = self.users.lock().expect("UserStore mutex poisoned");
            users.get(username).cloned()
        };
        if let Some(stored) = existing_hash {
            return verify_bcrypt(password.to_string(), stored).await.and_then(|ok| {
                if ok {
                    Ok(UpsertOutcome::LoggedIn)
                } else {
                    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
                }
            });
        }

        // Brand-new user — check the registration cap before doing
        // the (expensive) bcrypt hash.
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                let current = self.users.lock().expect("UserStore mutex poisoned").len() as u64;
                if current >= max {
                    return Err(RegistryError::TooManyUsers { max });
                }
            }
            MaxUsers::Unlimited => {}
        }

        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let snapshot = {
            let mut users = self.users.lock().expect("UserStore mutex poisoned");
            // Re-check the cap under the lock to make the limit hold
            // under concurrent adduser bursts. A second writer that
            // raced in while we were hashing could otherwise push
            // past the cap.
            if let MaxUsers::Limited(max) = self.max_users
                && (users.len() as u64) >= max
                && !users.contains_key(username)
            {
                return Err(RegistryError::TooManyUsers { max });
            }
            users.insert(username.to_string(), hash);
            serialize_htpasswd(&users)
        };
        self.persist(snapshot).await?;
        Ok(UpsertOutcome::Created)
    }

    /// Verify a username+password pair against the store. Returns
    /// `Some(username)` when the credentials match, `None`
    /// otherwise. Used by the Basic-auth path of [`identify`] —
    /// kept synchronous so `enforce_access` can stay sync.
    ///
    /// Bcrypt verification runs inline on the caller's task; at
    /// cost 10 it's ~50–100 ms, which is fine for the rare Basic
    /// path. The hot path is Bearer, which doesn't bcrypt.
    pub fn verify(&self, username: &str, password: &str) -> Option<String> {
        let stored = {
            let users = self.users.lock().expect("UserStore mutex poisoned");
            users.get(username).cloned()?
        };
        bcrypt::verify(password, &stored).ok()?.then(|| username.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpsertOutcome {
    /// The user didn't exist; we created the account.
    Created,
    /// The user existed and the password matched.
    LoggedIn,
}

impl UserStore {
    async fn persist(&self, body: String) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || write_atomic(&path, body.as_bytes())).await??;
        Ok(())
    }
}

/// SHA-256-hashed (token_hash → username) map, optionally backed by
/// a SQLite database for cross-restart durability.
///
/// Token records carry the verdaccio shape (created_at, last_used_at,
/// readonly, cidr_whitelist) so they can be surfaced by future
/// `/-/npm/v1/tokens` endpoints without a schema migration.
#[derive(Debug)]
pub struct TokenStore {
    inner: Mutex<TokenInner>,
    persist: Option<PathBuf>,
    secret: [u8; 32],
    counter: AtomicU64,
}

#[derive(Debug)]
struct TokenInner {
    /// hex-encoded SHA-256 of the raw token → record.
    tokens: HashMap<String, TokenRecord>,
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

    /// Mint a fresh token for `username`, persist its hash, and
    /// return the raw token to the caller. The raw token is never
    /// stored.
    pub async fn issue(&self, username: &str) -> Result<String> {
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
            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = Connection::open(&path)?;
                upsert_token(&conn, &hash_for_db, &record)?;
                Ok(())
            })
            .await??;
        }
        Ok(raw)
    }

    /// Resolve a raw token back to its username, if it was ever
    /// issued (and not since deleted). Runs entirely in memory.
    pub fn lookup(&self, raw: &str) -> Option<String> {
        let token_hash = sha256_hex(raw.as_bytes());
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        inner.tokens.get(&token_hash).map(|record| record.username.clone())
    }

    /// Snapshot the record for a token by its key (SHA-256 hex). Used
    /// to check ownership before revocation — the revoke handler
    /// rejects a delete from a non-owner with 403 before touching the
    /// store.
    pub fn find_by_key(&self, key: &str) -> Option<TokenRecord> {
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        inner.tokens.get(key).cloned()
    }

    /// All tokens owned by `username`. Returns `(key, record)` pairs
    /// where `key` is the SHA-256 hex digest of the raw token — the
    /// same value the `/-/npm/v1/tokens` listing surfaces, and what
    /// `npm token revoke` sends back to the delete endpoint.
    pub fn list_for_user(&self, username: &str) -> Vec<(String, TokenRecord)> {
        let inner = self.inner.lock().expect("TokenStore mutex poisoned");
        inner
            .tokens
            .iter()
            .filter(|(_, record)| record.username == username)
            .map(|(hash, record)| (hash.clone(), record.clone()))
            .collect()
    }

    /// Remove a token by its key (the SHA-256 hex digest). Returns
    /// the deleted record so the caller can check ownership before
    /// committing the revocation in a higher layer.
    ///
    /// SQLite gets the `DELETE` *before* the in-memory map is mutated.
    /// If the disk write fails, both views still hold the token and
    /// the caller sees a 5xx — the opposite ordering would leave a
    /// "revoked in memory but resurrected on restart" hole.
    pub async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
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

    /// Remove a token by its raw value. The `DELETE /-/user/token/:tok`
    /// (npm logout) path puts the bearer token verbatim in the URL,
    /// so this hashes first and then defers to [`Self::revoke_by_key`].
    pub async fn revoke_by_raw(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let key = sha256_hex(raw.as_bytes());
        self.revoke_by_key(&key).await
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl Default for UserStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

/// Identify the caller behind an HTTP request. Inspects the
/// `Authorization` header and resolves it to a username via the
/// token store (for `Bearer`) or the user store (for `Basic`).
/// Returns `None` for missing/unsupported credentials so the caller
/// can decide whether anonymous is allowed.
///
/// The scheme is matched case-insensitively (RFC 7235 §2.1: "the
/// scheme is case-insensitive"), so `BEARER`, `bearer`, and `Bearer`
/// all parse the same.
pub fn identify(
    header_value: Option<&str>,
    users: &UserStore,
    tokens: &TokenStore,
) -> Option<String> {
    let value = header_value?.trim();
    let mut parts = value.splitn(2, ' ');
    let scheme = parts.next()?;
    let credentials = parts.next()?.trim();
    if scheme.eq_ignore_ascii_case("Bearer") {
        return tokens.lookup(credentials);
    }
    if scheme.eq_ignore_ascii_case("Basic") {
        let decoded = BASE64.decode(credentials).ok()?;
        let pair = std::str::from_utf8(&decoded).ok()?;
        let (user, password) = pair.split_once(':')?;
        return users.verify(user, password);
    }
    None
}

// ---------------------------------------------------------------
// htpasswd I/O
// ---------------------------------------------------------------

/// Parse an Apache-shaped htpasswd file. Each non-empty, non-comment
/// line is `username:hash`; we accept any bcrypt variant (`$2a$`,
/// `$2b$`, `$2y$`) but reject everything else so a config file
/// holding `crypt(3)` or plaintext entries can't masquerade as
/// passing without the password actually being verifiable.
fn parse_htpasswd(raw: &str) -> std::result::Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches(['\r']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((user, hash)) = line.split_once(':') else {
            return Err(format!("line {}: missing ':' separator", line_no + 1));
        };
        let user = user.trim();
        let hash = hash.trim();
        if user.is_empty() {
            return Err(format!("line {}: empty username", line_no + 1));
        }
        if !is_supported_hash(hash) {
            return Err(format!(
                "line {}: unsupported hash format for user {user:?} (only bcrypt is accepted)",
                line_no + 1,
            ));
        }
        out.insert(user.to_string(), hash.to_string());
    }
    Ok(out)
}

/// True for any bcrypt variant. We don't accept `{SHA}`, `$apr1$`,
/// crypt(3), or plaintext — every supported entry must go through
/// `bcrypt::verify` cleanly.
fn is_supported_hash(hash: &str) -> bool {
    hash.starts_with("$2a$") || hash.starts_with("$2b$") || hash.starts_with("$2y$")
}

/// Serialize the user map back to htpasswd shape. Sorted output so
/// the file is stable under `git diff` and easier to eyeball.
fn serialize_htpasswd(users: &HashMap<String, String>) -> String {
    let mut entries: Vec<(&String, &String)> = users.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    let mut out = String::new();
    for (user, hash) in entries {
        out.push_str(user);
        out.push(':');
        out.push_str(hash);
        out.push('\n');
    }
    out
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = unique_tmp_path(path);
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn unique_tmp_path(base: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut name = base.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".tmp.{pid}.{counter}"));
    match base.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

// ---------------------------------------------------------------
// bcrypt helpers
// ---------------------------------------------------------------

/// Hash a password off the reactor — bcrypt at cost 10 takes
/// ~50–100 ms and stalls every other async task on the same thread
/// if run inline.
async fn hash_bcrypt(password: String, cost: u32) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let parts = bcrypt::hash_with_result(&password, cost)?;
        // Format as $2y$ for maximum cross-tool compatibility —
        // Apache's `htpasswd -B` writes $2y$, GNU coreutils tools
        // accept it, and bcrypt::verify reads any of $2a/$2b/$2y.
        Ok(parts.format_for_version(bcrypt::Version::TwoY))
    })
    .await?
}

async fn verify_bcrypt(password: String, hash: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password, &hash).map_err(RegistryError::from)
    })
    .await?
}

// ---------------------------------------------------------------
// SQLite-backed token store
// ---------------------------------------------------------------

fn init_tokens_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tokens (
             token_hash      TEXT PRIMARY KEY,
             username        TEXT NOT NULL,
             created_at      INTEGER NOT NULL,
             last_used_at    INTEGER NOT NULL,
             readonly        INTEGER NOT NULL DEFAULT 0,
             cidr_whitelist  TEXT NOT NULL DEFAULT '[]'
         );
         CREATE INDEX IF NOT EXISTS tokens_username ON tokens(username);",
    )?;
    Ok(())
}

fn load_all_tokens(conn: &Connection) -> Result<HashMap<String, TokenRecord>> {
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
        let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json).unwrap_or_default();
        out.insert(
            hash,
            TokenRecord {
                username,
                created_at: created_at as u64,
                last_used_at: last_used_at as u64,
                readonly: readonly != 0,
                cidr_whitelist,
            },
        );
    }
    Ok(out)
}

fn delete_token(conn: &Connection, token_hash: &str) -> Result<()> {
    conn.execute("DELETE FROM tokens WHERE token_hash = ?1", rusqlite::params![token_hash])?;
    Ok(())
}

fn upsert_token(conn: &Connection, token_hash: &str, record: &TokenRecord) -> Result<()> {
    let cidr_json = serde_json::to_string(&record.cidr_whitelist)
        .expect("Vec<String> always serializes to JSON");
    conn.execute(
        "INSERT INTO tokens (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(token_hash) DO UPDATE SET
            username = excluded.username,
            last_used_at = excluded.last_used_at,
            readonly = excluded.readonly,
            cidr_whitelist = excluded.cidr_whitelist",
        rusqlite::params![
            token_hash,
            record.username,
            record.created_at as i64,
            record.last_used_at as i64,
            record.readonly as i64,
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
fn fresh_secret() -> [u8; 32] {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).expect("OS CSPRNG must be available");
    secret
}

fn mint_token(secret: &[u8; 32], nonce: u64, username: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret);
    hasher.update(nonce.to_le_bytes());
    hasher.update(username.as_bytes());
    let digest = hasher.finalize();
    // 16 bytes of hash → 32 hex chars. Long enough to be
    // unguessable, short enough to keep test logs readable.
    hex_encode(&digest[..16])
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests;
