//! User and token storage for the registry.
//!
//! Auth state is split into two record stores, each behind a narrow
//! async trait so the backing store is config-selectable:
//!
//! * [`UserBackend`] — username → bcrypt-hashed password.
//! * [`TokenBackend`] — SHA-256 token hash → token record.
//!
//! Three implementations exist, picked at startup by
//! [`AuthState::load`]:
//!
//! * [`UserStore`] / [`TokenStore`] — the local default. Users are an
//!   Apache-style htpasswd file; tokens a `SQLite` database. Each keeps
//!   a full mirror of its state in a `Mutex<...>` and persists on
//!   every write, so reads (the hot path for `enforce_access`) never
//!   touch disk. With no file configured both fall back to a pure
//!   in-memory map (the `@pnpm/registry-mock` shape).
//! * [`LibsqlAuth`] — a networked-SQLite (libsql / Turso) backend that
//!   stores both records in a shared database, so several stateless
//!   pnpr replicas observe a consistent set of users and tokens. The
//!   on-disk htpasswd format doesn't network, so users live in a
//!   `users` table here; the `tokens` table matches the local schema.
//!
//! The raw token is only ever returned to the caller once on `issue`;
//! only its SHA-256 hash hits storage, so a leak of the database
//! doesn't grant access on its own.

use crate::{
    config::{AuthConfig, BackendConfig, MaxUsers},
    error::{RegistryError, Result},
};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use libsql_backend::LibsqlAuth;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

mod libsql_backend;

/// Bundle of the user store and the token store, each a trait object
/// so the rest of the server doesn't have to know whether auth is
/// file-backed, in-memory, or networked. Built once at startup by
/// [`Self::load`].
#[derive(Clone)]
pub struct AuthState {
    pub users: Arc<dyn UserBackend>,
    pub tokens: Arc<dyn TokenBackend>,
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AuthState").finish_non_exhaustive()
    }
}

impl AuthState {
    /// All-in-memory auth state. Used when no record backend is
    /// configured and neither `auth.htpasswd.file` nor
    /// `auth.tokens.file` are set, and by tests that don't care about
    /// persistence.
    #[must_use]
    pub fn in_memory() -> Self {
        Self { users: Arc::new(UserStore::in_memory()), tokens: Arc::new(TokenStore::in_memory()) }
    }

    /// Build the auth state from the resolved config. The networked
    /// [`BackendConfig::Libsql`] backs both stores with one shared
    /// database; otherwise each local store is in-memory when its file
    /// path is unset and file-backed otherwise. The fallible step (open
    /// the htpasswd / `SQLite` file, or connect to the networked DB and
    /// ensure its schema) runs here so a malformed file or an
    /// unreachable database surfaces as a startup error before the
    /// socket is bound.
    pub async fn load(auth: &AuthConfig, backend: &BackendConfig) -> Result<Self> {
        if let BackendConfig::Libsql(settings) = backend {
            let shared = Arc::new(LibsqlAuth::connect(settings, auth.htpasswd.max_users).await?);
            let users: Arc<dyn UserBackend> = Arc::clone(&shared) as Arc<dyn UserBackend>;
            let tokens: Arc<dyn TokenBackend> = shared;
            return Ok(Self { users, tokens });
        }
        let users: Arc<dyn UserBackend> = match auth.htpasswd.file.clone() {
            Some(path) => Arc::new(UserStore::open(path, auth.htpasswd.max_users)?),
            None => Arc::new(UserStore::in_memory()),
        };
        let tokens: Arc<dyn TokenBackend> = match auth.tokens.file.clone() {
            Some(path) => Arc::new(TokenStore::open(path)?),
            None => Arc::new(TokenStore::in_memory()),
        };
        Ok(Self { users, tokens })
    }
}

/// Username + password record store. The hot read is
/// [`Self::verify`] (the Basic-auth path of [`identify`]); the write
/// is [`Self::add_or_login`] (npm `adduser` / `login`).
#[async_trait]
pub trait UserBackend: Send + Sync {
    /// Add a new user or verify a returning one. See
    /// [`UpsertOutcome`] for the success cases; a wrong password for
    /// an existing user is [`RegistryError::Unauthenticated`], and a
    /// new user past the registration cap is
    /// [`RegistryError::RegistrationDisabled`] / `TooManyUsers`.
    async fn add_or_login(&self, username: &str, password: &str) -> Result<UpsertOutcome>;

    /// Verify a username+password pair. `Ok(Some(username))` on a match,
    /// `Ok(None)` when the user is unknown or the password is wrong, and
    /// `Err` only when the backing store itself fails — so a store
    /// outage surfaces as a 5xx rather than masquerading as a bad
    /// password.
    async fn verify(&self, username: &str, password: &str) -> Result<Option<String>>;
}

/// Bearer-token record store. The hot read is [`Self::lookup`]
/// (resolving the `Authorization: Bearer` header on nearly every
/// request); the rest back the `/-/npm/v1/tokens` CRUD endpoints.
#[async_trait]
pub trait TokenBackend: Send + Sync {
    /// Mint a fresh token for `username`, persist its hash, and return
    /// the raw token. The raw token is never stored.
    async fn issue(&self, username: &str) -> Result<String>;

    /// Resolve a raw token back to its username. `Ok(None)` means the
    /// token was never issued (or was revoked); `Err` means the backing
    /// store failed — never conflate the two, or a store outage reads as
    /// "not authenticated".
    async fn lookup(&self, raw: &str) -> Result<Option<String>>;

    /// Snapshot the record for a token by its key (SHA-256 hex). Used
    /// to check ownership before revocation. `Ok(None)` if no such
    /// token; `Err` on a store failure.
    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>>;

    /// All tokens owned by `username`, as `(key, record)` pairs where
    /// `key` is the SHA-256 hex digest the listing endpoint surfaces.
    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>>;

    /// Remove a token by its key (the SHA-256 hex digest). Returns the
    /// deleted record so a higher layer can confirm the revocation.
    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>>;

    /// Remove a token by its raw value — the `DELETE /-/user/token/:tok`
    /// (npm logout) path puts the bearer token verbatim in the URL, so
    /// this hashes first and defers to [`Self::revoke_by_key`].
    async fn revoke_by_raw(&self, raw: &str) -> Result<Option<TokenRecord>> {
        self.revoke_by_key(&sha256_hex(raw.as_bytes())).await
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
    #[must_use]
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
    async fn add_or_login(&self, username: &str, password: &str) -> Result<UpsertOutcome> {
        let existing_hash = {
            let users = self.users.lock().expect("UserStore mutex poisoned");
            users.get(username).cloned()
        };
        if let Some(stored) = existing_hash {
            return verify_returning_user(username, password, stored).await;
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
        enum NextStep {
            Persist(String),
            VerifyExisting(String),
        }
        let next_step = {
            let mut users = self.users.lock().expect("UserStore mutex poisoned");
            if let Some(stored) = users.get(username).cloned() {
                NextStep::VerifyExisting(stored)
            } else {
                // Re-check the cap under the lock to make the limit hold
                // under concurrent adduser bursts. A second writer that
                // raced in while we were hashing could otherwise push
                // past the cap.
                if let MaxUsers::Limited(max) = self.max_users
                    && (users.len() as u64) >= max
                {
                    return Err(RegistryError::TooManyUsers { max });
                }
                users.insert(username.to_string(), hash);
                NextStep::Persist(serialize_htpasswd(&users))
            }
        };
        match next_step {
            NextStep::Persist(snapshot) => {
                self.persist(snapshot).await?;
                Ok(UpsertOutcome::Created)
            }
            NextStep::VerifyExisting(stored) => {
                verify_returning_user(username, password, stored).await
            }
        }
    }

    async fn verify(&self, username: &str, password: &str) -> Result<Option<String>> {
        let stored = {
            let users = self.users.lock().expect("UserStore mutex poisoned");
            users.get(username).cloned()
        };
        let Some(stored) = stored else {
            return Ok(None);
        };
        // The in-memory read can't fail; a bcrypt error is treated as a
        // non-match (not a store outage), so it stays `Ok(None)`.
        Ok(verify_bcrypt(password.to_string(), stored)
            .await
            .unwrap_or(false)
            .then(|| username.to_string()))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpsertOutcome {
    /// The user didn't exist; we created the account.
    Created,
    /// The user existed and the password matched.
    LoggedIn,
}

/// SHA-256-hashed (`token_hash` → username) map, optionally backed by
/// a `SQLite` database for cross-restart durability.
///
/// Token records carry the verdaccio shape (`created_at`, `last_used_at`,
/// readonly, `cidr_whitelist`) so they can be surfaced by future
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
            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = Connection::open(&path)?;
                upsert_token(&conn, &hash_for_db, &record)?;
                Ok(())
            })
            .await??;
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
/// `Ok(None)` covers every "no usable credentials" case — a missing or
/// malformed header, an unsupported scheme, or credentials that simply
/// don't match. `Err` is reserved for a failure of the backing store
/// (e.g. the networked auth DB is unreachable) so the caller can return
/// a 5xx instead of a misleading 401.
pub async fn identify(
    header_value: Option<&str>,
    users: &dyn UserBackend,
    tokens: &dyn TokenBackend,
) -> Result<Option<String>> {
    let Some(value) = header_value.map(str::trim) else {
        return Ok(None);
    };
    let mut parts = value.splitn(2, ' ');
    let Some(scheme) = parts.next() else {
        return Ok(None);
    };
    let Some(credentials) = parts.next().map(str::trim) else {
        return Ok(None);
    };
    if scheme.eq_ignore_ascii_case("Bearer") {
        return tokens.lookup(credentials).await;
    }
    if scheme.eq_ignore_ascii_case("Basic") {
        let Ok(decoded) = BASE64.decode(credentials) else {
            return Ok(None);
        };
        let Ok(pair) = std::str::from_utf8(&decoded) else {
            return Ok(None);
        };
        let Some((user, password)) = pair.split_once(':') else {
            return Ok(None);
        };
        return users.verify(user, password).await;
    }
    Ok(None)
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
    let mut name = base.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
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

/// Verify `password` against an existing user's `stored` hash,
/// mapping the result to the login outcome a returning user expects:
/// `LoggedIn` on a match, `Unauthenticated` otherwise.
async fn verify_returning_user(
    username: &str,
    password: &str,
    stored: String,
) -> Result<UpsertOutcome> {
    if verify_bcrypt(password.to_string(), stored).await? {
        Ok(UpsertOutcome::LoggedIn)
    } else {
        Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
    }
}

// ---------------------------------------------------------------
// SQLite-backed token store
// ---------------------------------------------------------------

/// `tokens` table DDL — shared verbatim by the local [`TokenStore`]
/// and the networked [`LibsqlAuth`] so the two backends store the
/// same shape and a database can be moved between them.
const TOKENS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS tokens (
    token_hash      TEXT PRIMARY KEY,
    username        TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    last_used_at    INTEGER NOT NULL,
    readonly        INTEGER NOT NULL DEFAULT 0,
    cidr_whitelist  TEXT NOT NULL DEFAULT '[]'
)";

const TOKENS_INDEX_SQL: &str = "CREATE INDEX IF NOT EXISTS tokens_username ON tokens(username)";

/// `users` table DDL — only the networked backend needs it, since the
/// local backend keeps users in an htpasswd file. One bcrypt hash per
/// username, the same `$2y$...` string the htpasswd file would hold.
const USERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS users (
    username     TEXT PRIMARY KEY,
    bcrypt_hash  TEXT NOT NULL
)";

fn init_tokens_schema(conn: &Connection) -> Result<()> {
    conn.execute(TOKENS_TABLE_SQL, [])?;
    conn.execute(TOKENS_INDEX_SQL, [])?;
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
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests;
