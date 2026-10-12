use std::{collections::HashMap, sync::Mutex};

use serde::{Deserialize, Serialize};

use super::{
    MAX_ENTRIES, OidcState, Result, SESSION_PREFIX, SessionUser, Utc, handoffs::Handoff,
    random_secret, rejected, unavailable,
};

/// Browser sign-ins on this process: the sessions pnpr issued, and the
/// handoff codes that carry a session to the web UI.
#[derive(Default)]
pub(super) struct BrowserSessions {
    pub(super) sessions: Mutex<HashMap<String, Session>>,
    pub(super) handoffs: Mutex<HashMap<String, Handoff>>,
}

pub(super) struct Session {
    user: SessionUser,
    pub(super) expires: i64,
}

pub struct LoginSession {
    pub token: String,
    pub expires: i64,
    pub returns_to: LoginReturn,
}

/// Where a browser goes after signing in.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoginReturn {
    /// A page that shows the token, to copy into an `.npmrc`.
    #[default]
    Token,
    /// The web UI, which redeems a [handoff code](OidcState::hand_off) for
    /// the token.
    Ui,
}

impl OidcState {
    /// Resolves only pnpr-issued browser sessions. Unknown or expired session tokens fail closed.
    pub fn session(&self, raw: &str) -> Result<Option<SessionUser>> {
        if !raw.starts_with(SESSION_PREFIX) {
            return Ok(None);
        }
        let now = Utc::now().timestamp();
        let mut sessions = self.browser.sessions.lock().expect("OIDC session mutex poisoned");
        let hash = super::super::sha256_hex(raw.as_bytes());
        if let Some(session) = sessions.get(&hash)
            && session.expires > now
        {
            return Ok(Some(session.user.clone()));
        }
        sessions.remove(&hash);
        Err(rejected())
    }

    pub fn revoke_session(&self, raw: &str) -> bool {
        self.browser.sessions
            .lock()
            .expect("OIDC session mutex poisoned")
            .remove(&super::super::sha256_hex(raw.as_bytes()))
            .is_some()
    }

    /// Revoke every browser session `username` holds on this process,
    /// returning how many there were.
    pub fn revoke_user_sessions(&self, username: &str) -> usize {
        let mut sessions = self.browser.sessions.lock().expect("OIDC session mutex poisoned");
        let before = sessions.len();
        sessions.retain(|_, session| session.user.username != username);
        before - sessions.len()
    }

    pub(super) fn issue_session(
        &self,
        user: SessionUser,
        expiration: i64,
        returns_to: LoginReturn,
    ) -> Result<LoginSession> {
        let now = Utc::now().timestamp();
        let expires = expiration.min(now + 3600);
        if expires <= now {
            return Err(rejected());
        }
        let token = format!("{SESSION_PREFIX}{}", random_secret()?);
        let mut sessions = self.browser.sessions.lock().expect("OIDC session mutex poisoned");
        sessions.retain(|_, session| session.expires > now);
        if sessions.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        sessions.insert(super::super::sha256_hex(token.as_bytes()), Session { user, expires });
        Ok(LoginSession { token, expires, returns_to })
    }
}
