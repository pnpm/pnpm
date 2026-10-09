use std::time::Duration;

use super::{
    LoginSession, MAX_ENTRIES, OidcState, Result, Utc, random_secret, rejected, unavailable,
};

/// How long a handoff code can be redeemed. The UI redeems it on the page the
/// sign-in redirects to.
pub const HANDOFF_TTL: Duration = Duration::from_mins(1);

pub(super) struct Handoff {
    session: LoginSession,
    expires: i64,
}

impl OidcState {
    /// A one-time code that [`redeem_handoff`](Self::redeem_handoff) trades for
    /// `session`, so the sign-in redirect to the web UI carries no token.
    pub fn hand_off(&self, session: LoginSession) -> Result<String> {
        let code = random_secret()?;
        let now = Utc::now().timestamp();
        let mut handoffs = self.browser.handoffs.lock().expect("OIDC handoff mutex poisoned");
        handoffs.retain(|_, handoff| handoff.expires > now);
        if handoffs.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        let expires = now + HANDOFF_TTL.as_secs() as i64;
        handoffs.insert(super::super::sha256_hex(code.as_bytes()), Handoff { session, expires });
        Ok(code)
    }

    /// The session a handoff code was issued for. A code works once and only
    /// before it expires.
    pub fn redeem_handoff(&self, code: &str) -> Result<LoginSession> {
        let now = Utc::now().timestamp();
        let hash = super::super::sha256_hex(code.as_bytes());
        self.browser.handoffs
            .lock()
            .expect("OIDC handoff mutex poisoned")
            .remove(&hash)
            .filter(|handoff| handoff.expires > now)
            .map(|handoff| handoff.session)
            .ok_or_else(rejected)
    }
}
