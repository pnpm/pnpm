#[cfg(test)]
pub(crate) mod tests;

use crate::{
    AccountCredential, AuthState, DEFAULT_BCRYPT_COST, NewToken, TokenRecord, UpsertOutcome,
    verify_returning_user,
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
        let before = self.users.credential(username).await?;
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
        expected: &AccountCredential,
        token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let issued = self.tokens.issue_token(user_id, token).await?;
        let current = self.users.credential(username).await;
        if matches!(&current, Ok(Some(cred)) if cred == expected) {
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
        before: Option<AccountCredential>,
    ) -> Result<()> {
        let current = self.users.credential(username).await?;
        let Some(current) = current else {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        };
        if before.as_ref() != Some(&current) {
            verify_returning_user(username, password, &current, DEFAULT_BCRYPT_COST).await?;
        }
        Ok(())
    }
}
