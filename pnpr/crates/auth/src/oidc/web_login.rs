use std::{collections::HashMap, time::Duration};

use super::{MAX_ENTRIES, OidcState, Result, Utc, random_secret, rejected, unavailable};

#[cfg(test)]
mod tests;

/// How long a web login session remains valid for browser authentication and CLI polling.
pub const WEB_LOGIN_TTL: Duration = Duration::from_mins(5);

/// Letters without vowels or look-alike pairs, as RFC 8628 section 6.1
/// suggests for codes a person types.
const USER_CODE_ALPHABET: &[u8; 20] = b"BCDFGHJKLMNPQRSTVWXZ";
const USER_CODE_LETTERS: usize = 8;

pub(super) enum WebLoginState {
    /// Waiting for a browser to enter the user code.
    Pending,
    /// A browser entered the user code and went to sign in.
    Claimed,
    Fulfilled {
        token: String,
    },
}

pub(super) struct WebLoginEntry {
    pub(super) state: WebLoginState,
    pub(super) expires: i64,
    pub(super) poll_hash: String,
    pub(super) code_hash: String,
}

/// Pending CLI web logins, keyed by a login ID that only the server and the
/// signed sign-in cookie of the browser that claimed it ever hold.
#[derive(Default)]
pub(super) struct WebLogins {
    pub(super) entries: HashMap<String, WebLoginEntry>,
    pub(super) poll_to_login: HashMap<String, String>,
    /// Unclaimed logins by the hash of their user code.
    pub(super) code_to_login: HashMap<String, String>,
}

impl WebLogins {
    pub(super) fn prune(&mut self, now: i64) {
        let poll_to_login = &mut self.poll_to_login;
        let code_to_login = &mut self.code_to_login;
        self.entries.retain(|_, entry| {
            if entry.expires <= now {
                poll_to_login.remove(&entry.poll_hash);
                code_to_login.remove(&entry.code_hash);
                false
            } else {
                true
            }
        });
    }
}

/// What the CLI receives for a web login: the secret it polls with, and the
/// code the user types in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLoginTokens {
    pub poll_token: String,
    /// Formatted as `XXXX-XXXX`.
    pub user_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebLoginPoll {
    Pending,
    Ready(String),
    NotFound,
}

impl OidcState {
    /// Creates a pending web login session for the CLI web login flow.
    pub fn create_web_login(&self) -> Result<WebLoginTokens> {
        let login_id = random_secret()?;
        let poll_token = random_secret()?;
        let letters = random_user_code_letters()?;
        let now = Utc::now().timestamp();
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        logins.prune(now);
        if logins.entries.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        let code_hash = super::super::sha256_hex(letters.as_bytes());
        if logins.code_to_login.contains_key(&code_hash) {
            return Err(unavailable());
        }
        let poll_hash = super::super::sha256_hex(poll_token.as_bytes());
        logins.entries.insert(
            login_id.clone(),
            WebLoginEntry {
                state: WebLoginState::Pending,
                expires: now + WEB_LOGIN_TTL.as_secs() as i64,
                poll_hash: poll_hash.clone(),
                code_hash: code_hash.clone(),
            },
        );
        logins.poll_to_login.insert(poll_hash, login_id.clone());
        logins.code_to_login.insert(code_hash, login_id);
        let (first, second) = letters.split_at(USER_CODE_LETTERS / 2);
        Ok(WebLoginTokens { poll_token, user_code: format!("{first}-{second}") })
    }

    /// Claims the pending web login whose user code a browser entered,
    /// returning its login ID. Case, spaces, and dashes in `user_code` are
    /// ignored. A code can be claimed once.
    pub fn claim_web_login(&self, user_code: &str) -> Option<String> {
        let letters = normalize_user_code(user_code)?;
        let now = Utc::now().timestamp();
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        let login_id = logins.code_to_login.remove(&super::super::sha256_hex(letters.as_bytes()))?;
        let entry = logins.entries.get_mut(&login_id)?;
        if entry.expires <= now || !matches!(entry.state, WebLoginState::Pending) {
            return None;
        }
        entry.state = WebLoginState::Claimed;
        Some(login_id)
    }

    /// Fulfills a claimed web login with the issued session token.
    pub fn fulfill_web_login(&self, login_id: &str, token: String) -> Result<()> {
        let now = Utc::now().timestamp();
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        if let Some(entry) = logins.entries.get_mut(login_id)
            && entry.expires > now
            && matches!(entry.state, WebLoginState::Claimed)
        {
            entry.state = WebLoginState::Fulfilled { token };
            return Ok(());
        }
        Err(rejected())
    }

    /// Polls a web login session using the poll token. Consumes the session when ready.
    pub fn poll_web_login(&self, poll_token: &str) -> WebLoginPoll {
        let now = Utc::now().timestamp();
        let poll_hash = super::super::sha256_hex(poll_token.as_bytes());
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        let Some(login_id) = logins.poll_to_login.get(&poll_hash).cloned() else {
            return WebLoginPoll::NotFound;
        };
        let Some(entry) = logins.entries.get(&login_id) else {
            logins.poll_to_login.remove(&poll_hash);
            return WebLoginPoll::NotFound;
        };
        if entry.expires <= now {
            let code_hash = entry.code_hash.clone();
            logins.entries.remove(&login_id);
            logins.poll_to_login.remove(&poll_hash);
            logins.code_to_login.remove(&code_hash);
            return WebLoginPoll::NotFound;
        }
        if !matches!(entry.state, WebLoginState::Fulfilled { .. }) {
            return WebLoginPoll::Pending;
        }
        logins.poll_to_login.remove(&poll_hash);
        match logins.entries.remove(&login_id).map(|entry| entry.state) {
            Some(WebLoginState::Fulfilled { token }) => WebLoginPoll::Ready(token),
            _ => WebLoginPoll::NotFound,
        }
    }
}

/// The letters of `user_code` in canonical form, or `None` when it cannot be
/// a user code.
fn normalize_user_code(user_code: &str) -> Option<String> {
    let letters: String = user_code
        .chars()
        .filter(|char| !char.is_whitespace() && *char != '-')
        .map(|char| char.to_ascii_uppercase())
        .collect();
    (letters.len() == USER_CODE_LETTERS
        && letters
            .bytes()
            .all(|byte| USER_CODE_ALPHABET.contains(&byte)))
    .then_some(letters)
}

/// [`USER_CODE_LETTERS`] letters from [`USER_CODE_ALPHABET`].
fn random_user_code_letters() -> Result<String> {
    // The largest multiple of the alphabet size that fits in a byte. Rejecting
    // bytes at or above it keeps every letter equally likely.
    const UNBIASED_LIMIT: u8 = 240;
    let mut letters = String::with_capacity(USER_CODE_LETTERS);
    while letters.len() < USER_CODE_LETTERS {
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).map_err(|_| unavailable())?;
        if byte[0] < UNBIASED_LIMIT {
            let index = usize::from(byte[0]) % USER_CODE_ALPHABET.len();
            letters.push(char::from(USER_CODE_ALPHABET[index]));
        }
    }
    Ok(letters)
}
