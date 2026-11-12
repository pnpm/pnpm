use crate::{
    AuthState, NewToken, TokenBackend, TokenRecord, TokenStore, UpsertOutcome, UserBackend,
    UserStore,
};
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::Result;
use std::sync::Arc;
use tokio::sync::Notify;

struct DelayedTokens {
    tokens: TokenStore,
    started: Notify,
    resume: Notify,
}

#[async_trait]
impl TokenBackend for DelayedTokens {
    async fn issue_token(&self, user_id: &str, token: NewToken) -> Result<(String, TokenRecord)> {
        self.started.notify_one();
        self.resume.notified().await;
        self.tokens.issue_token(user_id, token).await
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        self.tokens.lookup(raw).await
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        self.tokens.find_by_key(key).await
    }

    async fn list_for_user(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>> {
        self.tokens.list_for_user(user_id).await
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        self.tokens.revoke_by_key(key).await
    }

    async fn delete_expired(&self, now: u64) -> Result<usize> {
        self.tokens.delete_expired(now).await
    }
}

#[tokio::test]
async fn a_password_reset_rejects_a_login_that_was_already_issuing_a_token() {
    let users = Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    let (_, id) = users.add_or_login("example-user", "old-password").await.unwrap();
    let tokens = Arc::new(DelayedTokens {
        tokens: TokenStore::in_memory(),
        started: Notify::new(),
        resume: Notify::new(),
    });
    let auth = Arc::new(AuthState { users, tokens: Arc::clone(&tokens) as Arc<dyn TokenBackend> });
    let login = {
        let auth = Arc::clone(&auth);
        tokio::spawn(async move { auth.password_login("example-user", "old-password").await })
    };
    tokens.started.notified().await;
    auth.change_password(&id, "new-password").await.unwrap();
    tokens.resume.notify_one();
    let error = login.await.unwrap().unwrap_err();
    assert_eq!(error.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    assert!(
        tokens
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn password_login_accepts_new_and_existing_accounts() {
    let auth = AuthState {
        users: Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4)),
        tokens: Arc::new(TokenStore::in_memory()),
    };
    let (outcome, id, token) = auth.password_login("example-user", "test-password").await.unwrap();
    assert_eq!(outcome, UpsertOutcome::Created);
    assert_eq!(
        auth.tokens
            .lookup(&token)
            .await
            .unwrap()
            .as_deref(),
        Some(id.as_str()),
    );
    let (outcome, login_id, token) =
        auth.password_login("example-user", "test-password").await.unwrap();
    assert_eq!(outcome, UpsertOutcome::LoggedIn);
    assert_eq!(id, login_id);
    assert_eq!(
        auth.tokens
            .lookup(&token)
            .await
            .unwrap()
            .as_deref(),
        Some(id.as_str()),
    );
}

#[tokio::test]
async fn password_login_accepts_a_credential_upgraded_during_verification() {
    let users = Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha1)
        .chain(b"test-password")
        .result();
    let digest = &integrity.hashes.first().unwrap().digest;
    users.insert_user_for_test("example-user".to_string(), format!("{{SHA}}{digest}"));
    let auth = AuthState {
        users: Arc::clone(&users) as Arc<dyn crate::UserBackend>,
        tokens: Arc::new(TokenStore::in_memory()),
    };
    let (outcome, id, token) = auth.password_login("example-user", "test-password").await.unwrap();
    assert_eq!(outcome, UpsertOutcome::LoggedIn);
    assert_eq!(
        auth.tokens
            .lookup(&token)
            .await
            .unwrap()
            .as_deref(),
        Some(id.as_str()),
    );
    assert!(crate::is_bcrypt_hash(
        users
            .credential("example-user")
            .await
            .unwrap()
            .unwrap()
            .password_hash()
            .unwrap()
    ));
}

#[tokio::test]
async fn token_creation_rejects_a_credential_reset_during_issuance() {
    let users = Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    let (_, id) = users.add_or_login("example-user", "old-password").await.unwrap();
    let verified_credential = users
        .credential("example-user")
        .await
        .unwrap()
        .unwrap();
    let tokens = Arc::new(DelayedTokens {
        tokens: TokenStore::in_memory(),
        started: Notify::new(),
        resume: Notify::new(),
    });
    let auth = Arc::new(AuthState { users, tokens: Arc::clone(&tokens) as Arc<dyn TokenBackend> });
    let expected_revision = auth
        .active_account_revision(&id)
        .await
        .unwrap()
        .unwrap();
    let issuance = {
        let auth = Arc::clone(&auth);
        let id = id.clone();
        tokio::spawn(async move {
            auth.issue_token_if_credential_current(
                "example-user",
                &id,
                &verified_credential,
                expected_revision,
                NewToken::default(),
            )
            .await
        })
    };
    tokens.started.notified().await;
    auth.change_password(&id, "new-password").await.unwrap();
    tokens.resume.notify_one();
    let error = issuance.await.unwrap().unwrap_err();
    assert_eq!(error.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    assert!(
        tokens
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn deactivation_during_login_or_token_creation_revokes_issued_token() {
    let users = Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    let (_, id) = users.add_or_login("example-user", "old-password").await.unwrap();
    let tokens = Arc::new(DelayedTokens {
        tokens: TokenStore::in_memory(),
        started: Notify::new(),
        resume: Notify::new(),
    });
    let auth = Arc::new(AuthState {
        users: Arc::clone(&users) as Arc<dyn UserBackend>,
        tokens: Arc::clone(&tokens) as Arc<dyn TokenBackend>,
    });

    let login = {
        let auth = Arc::clone(&auth);
        tokio::spawn(async move { auth.password_login("example-user", "old-password").await })
    };
    tokens.started.notified().await;
    auth.apply_account_status(&id, 1, crate::AccountLifecycle::Deactivated)
        .await
        .unwrap();
    auth.apply_account_status(&id, 2, crate::AccountLifecycle::Active).await.unwrap();
    users.set_password(&id, "old-password").await.unwrap();
    tokens.resume.notify_one();
    let login_err = login.await.unwrap().unwrap_err();
    assert_eq!(login_err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    assert!(
        tokens
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );

    users.clear_password(&id).await.unwrap();
    let expected_revision = auth
        .active_account_revision(&id)
        .await
        .unwrap()
        .unwrap();
    let issuance = {
        let auth = Arc::clone(&auth);
        let id = id.clone();
        tokio::spawn(async move {
            auth.issue_token_if_credential_current(
                "example-user",
                &id,
                &crate::AccountCredential::None,
                expected_revision,
                NewToken::default(),
            )
            .await
        })
    };
    tokens.started.notified().await;
    auth.apply_account_status(&id, 3, crate::AccountLifecycle::Deactivated)
        .await
        .unwrap();
    auth.apply_account_status(&id, 4, crate::AccountLifecycle::Active).await.unwrap();
    tokens.resume.notify_one();
    let issue_err = issuance.await.unwrap().unwrap_err();
    assert_eq!(issue_err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    assert!(
        tokens
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );
}

pub(crate) async fn assert_password_update_keeps_concurrent_resets(users: &dyn UserBackend) {
    users.create_user("example-user", "initial-password").await.unwrap();
    let id = users
        .account_id("example-user")
        .await
        .unwrap()
        .unwrap();
    let verified_hash = users
        .credential("example-user")
        .await
        .unwrap()
        .unwrap()
        .into_password_hash()
        .unwrap();
    users.set_password(&id, "admin-reset-password").await.unwrap();
    assert!(
        !users.set_password_if_current(&id, "attacker-password", &verified_hash).await.unwrap(),
    );
    assert!(users.add_or_login("example-user", "admin-reset-password").await.is_ok());
    assert!(users.add_or_login("example-user", "attacker-password").await.is_err());
    let current = users
        .credential("example-user")
        .await
        .unwrap()
        .unwrap()
        .into_password_hash()
        .unwrap();
    assert!(users.set_password_if_current(&id, "changed-password", &current).await.unwrap());
    assert!(users.add_or_login("example-user", "changed-password").await.is_ok());
}

#[tokio::test]
async fn local_password_updates_preserve_a_concurrent_admin_reset() {
    let users = UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4);
    assert_password_update_keeps_concurrent_resets(&users).await;
}
