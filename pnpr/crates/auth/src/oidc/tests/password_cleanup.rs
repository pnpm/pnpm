use super::super::{LoginReturn, OidcState, SessionUser};
use crate::{AuthState, NewToken, TokenBackend, TokenRecord, TokenStore, UserBackend, UserStore};
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

struct FailedRevocation(TokenStore);

#[async_trait]
impl TokenBackend for FailedRevocation {
    async fn issue_token(&self, id: &str, token: NewToken) -> Result<(String, TokenRecord)> {
        self.0.issue_token(id, token).await
    }
    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        self.0.lookup(raw).await
    }
    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        self.0.find_by_key(key).await
    }
    async fn list_for_user(&self, id: &str) -> Result<Vec<(String, TokenRecord)>> {
        self.0.list_for_user(id).await
    }
    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        self.0.revoke_by_key(key).await
    }
    async fn revoke_all_for_user(&self, _id: &str) -> Result<usize> {
        Err(RegistryError::StorageUnavailable { reason: "token store unavailable".to_string() })
    }
    async fn delete_expired(&self, now: u64) -> Result<usize> {
        self.0.delete_expired(now).await
    }
}

#[tokio::test]
async fn a_committed_password_error_revokes_browser_sessions_when_token_cleanup_fails() {
    let users = Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    users.create_user("example-user", "old-password").await.unwrap();
    let id = users
        .account_id("example-user")
        .await
        .unwrap()
        .unwrap();
    let hash = users
        .credential("example-user")
        .await
        .unwrap()
        .unwrap()
        .into_password_hash()
        .unwrap();
    let auth = AuthState { users, tokens: Arc::new(FailedRevocation(TokenStore::in_memory())) };
    let oidc = OidcState::new(&[], "https://example.test").unwrap();
    let session = oidc
        .issue_session(
            SessionUser { user_id: id.clone(), teams: Vec::new() },
            chrono::Utc::now().timestamp() + 60,
            LoginReturn::Token,
        )
        .unwrap();
    assert!(
        oidc.session(&session.token)
            .unwrap()
            .is_some(),
    );
    let invalid = auth.change_password_if_current(&id, "short", &hash).await.unwrap_err();
    assert_eq!(oidc.revoke_committed_password_sessions(&invalid), 0);
    assert!(
        oidc.session(&session.token)
            .unwrap()
            .is_some(),
    );
    let error = auth.change_password_if_current(&id, "new-password", &hash).await.unwrap_err();
    assert_eq!(error.committed_password_user(), Some(id.as_str()));
    assert_eq!(error.status_code(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(oidc.revoke_committed_password_sessions(&error), 1);
    assert!(oidc.session(&session.token).is_err());
    assert!(auth.users.add_or_login("example-user", "new-password").await.is_ok());
}
