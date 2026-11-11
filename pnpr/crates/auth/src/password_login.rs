#[cfg(test)]
pub(crate) mod tests;

use crate::{
    AuthState, DEFAULT_BCRYPT_COST, NewToken, TokenRecord, UpsertOutcome,
    htpasswd::verify_returning_user,
};
use pnpr_error::{RegistryError, Result};

impl AuthState {
    /// Verify or register an account and issue a token. A password changed
    /// during token issuance must still match before the token is returned.
    pub async fn password_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String, String)> {
        let before = self.users.password_hash(username).await?;
        let (outcome, id) = self.users.add_or_login(username, password).await?;
        let token = self.tokens.issue(&id).await?;
        if let Err(error) = self.check_login_password(username, password, before).await {
            self.tokens.revoke_by_raw(&token).await?;
            return Err(error);
        }
        Ok((outcome, id, token))
    }

    /// Issue a token only while the credential verified by the caller remains current.
    pub async fn issue_token_if_credential_current(
        &self,
        username: &str,
        user_id: &str,
        expected_hash: &str,
        token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let issued = self.tokens.issue_token(user_id, token).await?;
        let current = self.users.password_hash(username).await;
        if matches!(&current, Ok(Some(hash)) if hash == expected_hash) {
            return Ok(issued);
        }
        self.tokens.revoke_by_raw(&issued.0).await?;
        let _ = current?;
        Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
    }

    async fn check_login_password(
        &self,
        username: &str,
        password: &str,
        before: Option<String>,
    ) -> Result<()> {
        let current = self.users.password_hash(username).await?;
        let Some(current) = current else {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        };
        if before.as_deref() != Some(current.as_str()) {
            verify_returning_user(username, password, current, DEFAULT_BCRYPT_COST).await?;
        }
        Ok(())
    }
}
