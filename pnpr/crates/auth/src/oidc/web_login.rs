use std::time::Duration;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebLoginPoll {
    Pending,
    Ready(String),
    NotFound,
}

impl OidcState {
    /// Creates a pending web login session for the CLI web login flow.
    pub fn create_web_login(&self) -> Result<String> {
        let session_id = random_secret()?;
        let now = Utc::now().timestamp();
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        logins.retain(|_, entry| entry.expires > now);
        if logins.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        let expires = now + WEB_LOGIN_TTL.as_secs() as i64;
        let hash = super::super::sha256_hex(session_id.as_bytes());
        logins.insert(hash, WebLoginEntry { state: WebLoginState::Pending, expires });
        Ok(session_id)
    }

    /// Checks if a web login session exists, is unexpired, and is still pending.
    pub fn is_valid_web_login(&self, session_id: &str) -> bool {
        let now = Utc::now().timestamp();
        let hash = super::super::sha256_hex(session_id.as_bytes());
        let logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        logins
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
        if let Some(entry) = logins.get_mut(&hash)
            && entry.expires > now
            && matches!(entry.state, WebLoginState::Pending)
        {
            entry.state = WebLoginState::Fulfilled { token };
            return Ok(());
        }
        Err(rejected())
    }

    /// Polls a web login session. Consumes the session when ready.
    pub fn poll_web_login(&self, session_id: &str) -> WebLoginPoll {
        let now = Utc::now().timestamp();
        let hash = super::super::sha256_hex(session_id.as_bytes());
        let mut logins = self.browser.web_logins.lock().expect("OIDC web login mutex poisoned");
        let Some(entry) = logins.get(&hash) else {
            return WebLoginPoll::NotFound;
        };
        if entry.expires <= now {
            logins.remove(&hash);
            return WebLoginPoll::NotFound;
        }
        if matches!(entry.state, WebLoginState::Pending) {
            return WebLoginPoll::Pending;
        }
        let Some(entry) = logins.remove(&hash) else {
            return WebLoginPoll::NotFound;
        };
        if let WebLoginState::Fulfilled { token } = entry.state {
            WebLoginPoll::Ready(token)
        } else {
            WebLoginPoll::NotFound
        }
    }
}
