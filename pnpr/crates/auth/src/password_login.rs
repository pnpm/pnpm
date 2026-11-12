#[cfg(test)]
pub(crate) mod tests;

use crate::{
    AccountCredential, AuthState, DEFAULT_BCRYPT_COST, NewToken, PasswordChange, TokenRecord,
    UpsertOutcome, UserCreation, UserPut, verify_returning_user,
};
use pnpr_error::{RegistryError, Result};

struct LoginSnapshot {
    credential: Option<AccountCredential>,
    account_id: Option<String>,
    revision: Option<u64>,
}

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

    /// Verify or register an account and issue `token` to it. A password
    /// changed or account deactivated during token issuance invalidates the
    /// new token.
    pub async fn password_login(
        &self,
        username: &str,
        password: &str,
        token: NewToken,
    ) -> Result<(UpsertOutcome, String, String)> {
        let mut snapshot = self.capture_login_snapshot(username).await?;
        let (outcome, id) = self.users.add_or_login(username, password).await?;
        if snapshot.revision.is_none() {
            snapshot.revision = match outcome {
                UpsertOutcome::Created => Some(0),
                UpsertOutcome::LoggedIn => self.active_account_revision(&id).await?,
            };
        }
        let (token, _) = self.tokens.issue_token(&id, token).await?;
        if let Err(error) = self.check_login_after_issue(&id, username, password, snapshot).await {
            self.tokens.revoke_by_raw(&token).await?;
            return Err(error);
        }
        Ok((outcome, id, token))
    }

    async fn capture_login_snapshot(&self, username: &str) -> Result<LoginSnapshot> {
        let credential = self.users.credential(username).await?;
        let account_id = self.users.active_account_id(username).await?;
        let revision = match &account_id {
            Some(existing_id) => self.active_account_revision(existing_id).await?,
            None => None,
        };
        Ok(LoginSnapshot { credential, account_id, revision })
    }

    /// Issue a token only while the account stays active at `expected_revision`
    /// and the verified credential remains current.
    pub async fn issue_token_if_credential_current(
        &self,
        username: &str,
        user_id: &str,
        expected: &AccountCredential,
        expected_revision: u64,
        token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let issued = self.tokens.issue_token(user_id, token).await?;
        let valid =
            self.is_issued_token_valid(username, user_id, expected, expected_revision).await;
        if matches!(valid, Ok(true)) {
            return Ok(issued);
        }
        self.tokens.revoke_by_raw(&issued.0).await?;
        let _ = valid?;
        Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
    }

    /// Status revision of `user_id` when active.
    pub async fn active_account_revision(&self, user_id: &str) -> Result<Option<u64>> {
        let id = user_id.to_string();
        let revisions = self.users.status_revisions(std::slice::from_ref(&id)).await?;
        Ok(revisions
            .get(&id)
            .filter(|rev| rev.status.is_active())
            .map(|rev| rev.revision))
    }

    async fn check_login_after_issue(
        &self,
        user_id: &str,
        username: &str,
        password: &str,
        snapshot: LoginSnapshot,
    ) -> Result<()> {
        if snapshot.account_id
            .as_deref()
            .is_some_and(|existing_id| existing_id != user_id)
        {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }
        let Some(expected_rev) = snapshot.revision else {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        };
        if self.active_account_revision(user_id).await? != Some(expected_rev)
            || self.users.active_account_id(username).await?.as_deref() != Some(user_id)
        {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }
        let Some(current) = self.users.credential(username).await? else {
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        };
        if snapshot.credential.as_ref() != Some(&current) {
            verify_returning_user(username, password, &current, DEFAULT_BCRYPT_COST).await?;
        }
        Ok(())
    }

    async fn is_issued_token_valid(
        &self,
        username: &str,
        user_id: &str,
        expected: &AccountCredential,
        expected_rev: u64,
    ) -> Result<bool> {
        if self.active_account_revision(user_id).await? != Some(expected_rev)
            || self.users.active_account_id(username).await?.as_deref() != Some(user_id)
        {
            return Ok(false);
        }
        let current = self.users.credential(username).await?;
        Ok(current.as_ref() == Some(expected))
    }
}
