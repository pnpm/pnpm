use std::{collections::HashMap, time::Duration};

use super::{MAX_ENTRIES, OidcState, Result, Utc, random_secret, rejected, unavailable};

#[cfg(test)]
mod tests;

/// How long a web login session remains valid for browser authentication and CLI polling.
pub const WEB_LOGIN_TTL: Duration = Duration::from_mins(5);

pub(super) enum WebLoginState {
    Pending,
    Fulfilled { token: String },
}

pub(super) struct WebLoginEntry {
    pub(super) state: WebLoginState,
    pub(super) expires: i64,
    pub(super) poll_hash: String,
}

#[derive(Default)]
pub(super) struct WebLogins {
    pub(super) entries: HashMap<String, WebLoginEntry>,
    pub(super) poll_to_browser: HashMap<String, String>,
}

impl WebLogins {
    pub(super) fn prune(&mut self, now: i64) {
        let poll_to_browser = &mut self.poll_to_browser;
        self.entries.retain(|_, entry| {
            if entry.expires <= now {
                poll_to_browser.remove(&entry.poll_hash);
                false
            } else {
                true
            }
        });
    }
}

/// The two secrets issued for a CLI web login flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLoginTokens {
    pub browser_session_id: String,
    pub poll_token: String,
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
        let browser_session_id = random_secret()?;
        let poll_token = random_secret()?;
        let now = Utc::now().timestamp();
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        logins.prune(now);
        if logins.entries.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        let expires = now + WEB_LOGIN_TTL.as_secs() as i64;
        let browser_hash = super::super::sha256_hex(browser_session_id.as_bytes());
        let poll_hash = super::super::sha256_hex(poll_token.as_bytes());
        logins.entries.insert(
            browser_hash.clone(),
            WebLoginEntry { state: WebLoginState::Pending, expires, poll_hash: poll_hash.clone() },
        );
        logins.poll_to_browser.insert(poll_hash, browser_hash);
        Ok(WebLoginTokens { browser_session_id, poll_token })
    }

    /// Checks if a web login session exists, is unexpired, and is still pending.
    pub fn is_valid_web_login(&self, session_id: &str) -> bool {
        let now = Utc::now().timestamp();
        let hash = super::super::sha256_hex(session_id.as_bytes());
        let logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        logins.entries
            .get(&hash)
            .is_some_and(|entry| {
                entry.expires > now && matches!(entry.state, WebLoginState::Pending)
            })
    }

    /// Fulfills a pending web login session with the issued session token.
    pub fn fulfill_web_login(&self, session_id: &str, token: String) -> Result<()> {
        let now = Utc::now().timestamp();
        let hash = super::super::sha256_hex(session_id.as_bytes());
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        if let Some(entry) = logins.entries.get_mut(&hash)
            && entry.expires > now
            && matches!(entry.state, WebLoginState::Pending)
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
        let Some(browser_hash) = logins.poll_to_browser.get(&poll_hash).cloned() else {
            return WebLoginPoll::NotFound;
        };
        let Some(entry) = logins.entries.get(&browser_hash) else {
            logins.poll_to_browser.remove(&poll_hash);
            return WebLoginPoll::NotFound;
        };
        if entry.expires <= now {
            logins.entries.remove(&browser_hash);
            logins.poll_to_browser.remove(&poll_hash);
            return WebLoginPoll::NotFound;
        }
        if matches!(entry.state, WebLoginState::Pending) {
            return WebLoginPoll::Pending;
        }
        let Some(entry) = logins.entries.remove(&browser_hash) else {
            logins.poll_to_browser.remove(&poll_hash);
            return WebLoginPoll::NotFound;
        };
        logins.poll_to_browser.remove(&poll_hash);
        if let WebLoginState::Fulfilled { token } = entry.state {
            WebLoginPoll::Ready(token)
        } else {
            WebLoginPoll::NotFound
        }
    }
}
