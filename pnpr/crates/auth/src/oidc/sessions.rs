use super::{
    LoginSession, MAX_ENTRIES, OidcState, Result, SESSION_PREFIX, Session, Utc, random_secret,
    rejected, unavailable,
};

impl OidcState {
    /// Resolves only pnpr-issued browser sessions. Unknown or expired session tokens fail closed.
    pub fn session(&self, raw: &str) -> Result<Option<String>> {
        if !raw.starts_with(SESSION_PREFIX) {
            return Ok(None);
        }
        let now = Utc::now().timestamp();
        let mut sessions = self.sessions.lock().expect("OIDC session mutex poisoned");
        let hash = super::super::sha256_hex(raw.as_bytes());
        if let Some(session) = sessions.get(&hash)
            && session.expires > now
        {
            return Ok(Some(session.username.clone()));
        }
        sessions.remove(&hash);
        Err(rejected())
    }

    pub fn revoke_session(&self, raw: &str) -> bool {
        self.sessions
            .lock()
            .expect("OIDC session mutex poisoned")
            .remove(&super::super::sha256_hex(raw.as_bytes()))
            .is_some()
    }

    pub(super) fn issue_session(&self, username: &str, expiration: i64) -> Result<LoginSession> {
        let now = Utc::now().timestamp();
        let expires = expiration.min(now + 3600);
        if expires <= now {
            return Err(rejected());
        }
        let token = format!("{SESSION_PREFIX}{}", random_secret()?);
        let mut sessions = self.sessions.lock().expect("OIDC session mutex poisoned");
        sessions.retain(|_, session| session.expires > now);
        if sessions.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        sessions.insert(
            super::super::sha256_hex(token.as_bytes()),
            Session { username: username.to_string(), expires },
        );
        Ok(LoginSession { token, expires })
    }
}
