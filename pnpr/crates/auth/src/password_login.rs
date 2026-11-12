#[cfg(test)]
pub(crate) mod tests;

use crate::{
    AccountCredential, AuthState, DEFAULT_BCRYPT_COST, NewToken, PasswordChange, TokenRecord,
    UpsertOutcome, UserCreation, UserPut, verify_returning_user,
};
use pnpr_error::{RegistryError, Result};

impl AuthState {
    /// Create an account with `username`, or change the existing account
    /// password. `None` means a concurrent writer removed the account.
    pub async fn put_user(&self, username: &str, password: &str) -> Result<Option<UserPut>> {
        match self.users.create_user(username, password).await? {
            UserCreation::Created(user_id) => Ok(Some(UserPut::Created { user_id })),
            UserCreation::NameTaken => {
                let Some(id) = self.users.account_id(username).await? else {
                    return Ok(None);
                };
                match self.change_password(&id, password).await? {
                    PasswordChange::Changed => Ok(Some(UserPut::PasswordChanged { user_id: id })),
                    // Another caller removed the user between the two calls.
                    PasswordChange::NoSuchUser => Ok(None),
                }
            }
        }
    }

    /// Replace a password and revoke every bearer token for the account.
    pub async fn change_password(&self, user_id: &str, password: &str) -> Result<PasswordChange> {
        let outcome = self.users.set_password(user_id, password).await;
        self.revoke_after_password_change(user_id, outcome, |change| {
            matches!(change, PasswordChange::Changed)
        })
        .await
    }

    /// Change a verified password without overwriting a concurrent reset.
    pub async fn change_password_if_current(
        &self,
        user_id: &str,
        password: &str,
        expected_hash: &str,
    ) -> Result<bool> {
        let outcome = self.users.set_password_if_current(user_id, password, expected_hash).await;
        self.revoke_after_password_change(user_id, outcome, |changed| *changed).await
    }

    async fn revoke_after_password_change<Outcome: Send>(
        &self,
        user_id: &str,
        outcome: Result<Outcome>,
        changed: impl FnOnce(&Outcome) -> bool + Send,
    ) -> Result<Outcome> {
        let committed = match &outcome {
            Ok(outcome) => changed(outcome),
            Err(error) => error.committed_password_user().is_some(),
        };
        if committed && let Err(source) = self.revoke_all_tokens(user_id).await {
            if let Err(original) = &outcome {
                tracing::error!(error = %source.log_message(), "Revoking tokens after a committed password change failed");
                tracing::error!(error = %original.log_message(), "The password change also failed to refresh its replica");
            }
            return Err(RegistryError::PasswordChangeCommitted {
                user_id: user_id.to_string(),
                source: Box::new(source),
            });
        }
        outcome
    }

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
