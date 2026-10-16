//! The login a redirect leaves pending: the payload sealed into the browser's
//! cookie, and the checks that let a callback spend it exactly once.

use super::{
    BASE64_URL_SAFE_NO_PAD, Deserialize, LoginReturn, MAX_ENTRIES, OidcState, Result, Serialize,
    Signature, Utc, rejected, unavailable,
};
use base64::Engine as _;
use p256::ecdsa::signature::{Signer as _, Verifier as _};

#[derive(Serialize, Deserialize)]
pub(super) struct PendingLogin {
    pub(super) provider: String,
    pub(super) nonce: String,
    pub(super) verifier: String,
    pub(super) state_hash: String,
    pub(super) expires: i64,
    #[serde(default)]
    pub(super) returns_to: LoginReturn,
}

impl OidcState {
    pub(super) fn seal_login(&self, login: &PendingLogin) -> Result<String> {
        let bytes = serde_json::to_vec(login).map_err(|_| unavailable())?;
        let signature: Signature = self.state_key.sign(&bytes);
        Ok(format!(
            "{}.{}",
            BASE64_URL_SAFE_NO_PAD.encode(bytes),
            BASE64_URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        ))
    }

    pub(super) fn open_login(&self, cookie: &str) -> Result<PendingLogin> {
        if cookie.len() > 4096 {
            return Err(rejected());
        }
        let (payload, signature) = cookie.split_once('.').ok_or_else(rejected)?;
        let payload = BASE64_URL_SAFE_NO_PAD.decode(payload).map_err(|_| rejected())?;
        let signature = BASE64_URL_SAFE_NO_PAD.decode(signature).map_err(|_| rejected())?;
        let signature = Signature::from_slice(&signature).map_err(|_| rejected())?;
        self.state_key
            .verifying_key()
            .verify(&payload, &signature)
            .map_err(|_| rejected())?;
        serde_json::from_slice(&payload).map_err(|_| rejected())
    }

    pub(super) fn validate_pending_login(
        &self,
        login: &PendingLogin,
        provider_name: &str,
        state: &str,
        code: &str,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        if login.provider != provider_name
            || login.expires <= now
            || login.state_hash != super::super::sha256_hex(state.as_bytes())
            || code.is_empty()
            || code.len() > 8192
        {
            return Err(rejected());
        }
        if self.consumed
            .lock()
            .expect("OIDC consumed mutex poisoned")
            .contains_key(&login.state_hash)
        {
            return Err(rejected());
        }
        Ok(())
    }

    pub(super) fn record_attempt(&self, state_hash: &str) -> Result<()> {
        let mut attempts = self.attempts.lock().expect("OIDC attempts mutex poisoned");
        if attempts
            .iter()
            .any(|state| state == state_hash)
        {
            return Err(rejected());
        }
        if attempts.len() >= MAX_ENTRIES {
            attempts.pop_front();
        }
        attempts.push_back(state_hash.to_string());
        Ok(())
    }
}
