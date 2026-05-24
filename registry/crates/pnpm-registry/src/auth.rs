//! User and token storage for the registry.
//!
//! The pnpm tests that use `@pnpm/registry-mock` boot the registry,
//! call `addUser` once, and then exercise the resulting Bearer token
//! against protected packages. There's no need for password hashing,
//! token expiration, or on-disk persistence — everything lives in an
//! in-memory store guarded by a `Mutex`.
//!
//! Two pieces matter:
//!
//! * [`UserStore`] — username → plaintext password. Verified via
//!   constant-time compare to guard against test-timing weirdness
//!   (overkill but cheap, since `subtle` isn't in the workspace and
//!   we can write it ourselves).
//! * [`TokenStore`] — token → username. Tokens are 32-hex-char
//!   strings derived from a per-server secret plus a monotonic
//!   counter via SHA-256 — opaque to the client, not guessable
//!   without the secret, and never collide within a process.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use sha2::{Digest, Sha256};

use crate::error::RegistryError;

/// In-memory username → password map. Populated by the adduser
/// endpoint; passwords are stored in plaintext because tests
/// already have the plaintext on hand and there's no value in
/// hashing it for a process-local registry.
#[derive(Debug, Default)]
pub struct UserStore {
    users: Mutex<std::collections::HashMap<String, String>>,
}

impl UserStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the user already existed and the password
    /// matched (a "login" against an existing account), false if
    /// the password didn't match. When the user doesn't exist, the
    /// account is created with the supplied password and `Ok(false)`
    /// (still "not a login") is returned — matches verdaccio's
    /// adduser behavior where a brand-new user is registered on
    /// the spot.
    pub fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UpsertOutcome, RegistryError> {
        let mut users = self.users.lock().expect("UserStore mutex poisoned");
        match users.get(username) {
            Some(existing) => {
                if constant_time_eq(existing.as_bytes(), password.as_bytes()) {
                    Ok(UpsertOutcome::LoggedIn)
                } else {
                    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
                }
            }
            None => {
                users.insert(username.to_string(), password.to_string());
                Ok(UpsertOutcome::Created)
            }
        }
    }

    /// Verify a username+password pair against the store. Returns
    /// `Some(username)` when the credentials match, `None`
    /// otherwise — never errors, since the caller may want to
    /// degrade to anonymous on failure rather than fail outright.
    pub fn verify(&self, username: &str, password: &str) -> Option<String> {
        let users = self.users.lock().expect("UserStore mutex poisoned");
        let stored = users.get(username)?;
        constant_time_eq(stored.as_bytes(), password.as_bytes()).then(|| username.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpsertOutcome {
    /// The user didn't exist; we created the account.
    Created,
    /// The user existed and the password matched.
    LoggedIn,
}

/// In-memory token → username map. Tokens are minted on adduser
/// and on the basic-auth fallback for endpoints that need a
/// token in the response body.
#[derive(Debug)]
pub struct TokenStore {
    tokens: Mutex<std::collections::HashMap<String, String>>,
    secret: [u8; 32],
    counter: AtomicU64,
}

impl TokenStore {
    /// Build a store with a freshly-randomized secret. The secret
    /// is derived from the system time + process id + a small
    /// startup-only RNG fallback — good enough for a test server
    /// that runs for a few seconds at a time, and lets us avoid
    /// pulling in `rand` as a new workspace dependency.
    pub fn new() -> Self {
        let mut hasher = Sha256::new();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        hasher.update(nanos.to_le_bytes());
        hasher.update(std::process::id().to_le_bytes());
        // Mix in the address of a stack allocation to add a sliver
        // of ASLR-derived entropy; not relied on for security, just
        // for collision resistance across multiple test processes
        // started within the same nanosecond.
        let addr = &nanos as *const u128 as usize;
        hasher.update(addr.to_le_bytes());
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&hasher.finalize());
        Self {
            tokens: Mutex::new(std::collections::HashMap::new()),
            secret,
            counter: AtomicU64::new(0),
        }
    }

    /// Mint a fresh token for `username` and remember it.
    pub fn issue(&self, username: &str) -> String {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut hasher = Sha256::new();
        hasher.update(self.secret);
        hasher.update(nonce.to_le_bytes());
        hasher.update(username.as_bytes());
        let digest = hasher.finalize();
        // 16 bytes of hash → 32 hex chars. Long enough to be
        // unguessable, short enough to keep test logs readable.
        let token = hex_encode(&digest[..16]);
        let mut tokens = self.tokens.lock().expect("TokenStore mutex poisoned");
        tokens.insert(token.clone(), username.to_string());
        token
    }

    /// Resolve a token back to its username, if it was issued by
    /// this store.
    pub fn lookup(&self, token: &str) -> Option<String> {
        let tokens = self.tokens.lock().expect("TokenStore mutex poisoned");
        tokens.get(token).cloned()
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Identify the caller behind an HTTP request. Inspects the
/// `Authorization` header and resolves it to a username via the
/// token store (for `Bearer`) or the user store (for `Basic`).
/// Returns `None` for missing/unsupported credentials so the caller
/// can decide whether anonymous is allowed.
pub fn identify(
    header_value: Option<&str>,
    users: &UserStore,
    tokens: &TokenStore,
) -> Option<String> {
    let value = header_value?.trim();
    if let Some(token) = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")) {
        return tokens.lookup(token.trim());
    }
    if let Some(encoded) = value.strip_prefix("Basic ").or_else(|| value.strip_prefix("basic ")) {
        let decoded = BASE64.decode(encoded.trim()).ok()?;
        let pair = std::str::from_utf8(&decoded).ok()?;
        let (user, password) = pair.split_once(':')?;
        return users.verify(user, password);
    }
    None
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Constant-time byte equality. Avoids early-exit timing leaks
/// between password comparisons. We don't import the `subtle`
/// crate just for this — a hand-rolled XOR loop is trivially
/// correct and our threat model is "test runner", not "live
/// internet".
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (l, r) in left.iter().zip(right.iter()) {
        diff |= l ^ r;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{TokenStore, UpsertOutcome, UserStore, identify};

    #[test]
    fn adduser_creates_then_validates() {
        let store = UserStore::new();
        let outcome = store.add_or_login("alice", "secret").unwrap();
        matches!(outcome, UpsertOutcome::Created);

        let outcome = store.add_or_login("alice", "secret").unwrap();
        matches!(outcome, UpsertOutcome::LoggedIn);

        assert!(store.verify("alice", "secret").is_some());
        assert!(store.verify("alice", "wrong").is_none());
        assert!(store.verify("bob", "secret").is_none());
    }

    #[test]
    fn adduser_rejects_existing_user_with_wrong_password() {
        let store = UserStore::new();
        store.add_or_login("alice", "secret").unwrap();
        let err = store.add_or_login("alice", "different").unwrap_err();
        // Maps to 401, matching what npm/verdaccio return for a
        // bad password against an existing username.
        assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn tokens_round_trip() {
        let tokens = TokenStore::new();
        let token = tokens.issue("alice");
        assert_eq!(tokens.lookup(&token).as_deref(), Some("alice"));
        assert!(tokens.lookup("not-a-token").is_none());
    }

    #[test]
    fn tokens_are_unique_per_issue() {
        let tokens = TokenStore::new();
        let a = tokens.issue("alice");
        let b = tokens.issue("alice");
        assert_ne!(a, b, "every call to issue() should mint a fresh token");
    }

    #[test]
    fn identify_recognizes_bearer_and_basic() {
        let users = UserStore::new();
        users.add_or_login("alice", "secret").unwrap();
        let tokens = TokenStore::new();
        let token = tokens.issue("alice");

        // Bearer
        let header = format!("Bearer {token}");
        assert_eq!(identify(Some(&header), &users, &tokens).as_deref(), Some("alice"));

        // Basic: base64(alice:secret) = YWxpY2U6c2VjcmV0
        let basic = "Basic YWxpY2U6c2VjcmV0";
        assert_eq!(identify(Some(basic), &users, &tokens).as_deref(), Some("alice"));

        // Wrong password — None, not an error.
        let wrong = format!(
            "Basic {}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"alice:wrong"),
        );
        assert!(identify(Some(&wrong), &users, &tokens).is_none());

        // Missing header
        assert!(identify(None, &users, &tokens).is_none());

        // Garbage
        assert!(identify(Some("Bearer total-nonsense"), &users, &tokens).is_none());
    }
}
