use super::{account_router, load_config, send};
use pnpr::{AuthState, TokenBackend, TokenStore, UserBackend, UserStore};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn a_passwordless_account_cannot_enroll_a_password_with_its_token() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    let id = users.provision_user("example-user").await.unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let token = tokens.issue(&id).await.unwrap();
    let auth = AuthState { users: Arc::clone(&users) as Arc<dyn UserBackend>, tokens };
    let app = account_router(load_config(&dir), auth).await;
    let update = json!({ "password": { "old": "anything", "new": "new-password" } });
    let (status, _) = send(&app, "POST", "/-/npm/v1/user", Some(&token), Some(update)).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert_eq!(
        users.credential("example-user").await.unwrap(),
        Some(pnpr_auth::AccountCredential::None),
    );
    let (status, _) = send(&app, "POST", "/-/npm/v1/tokens", Some(&token), Some(json!({}))).await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

struct PausedPasswordUpdate {
    users: Arc<UserStore>,
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl UserBackend for PausedPasswordUpdate {
    async fn add_or_login(
        &self,
        name: &str,
        password: &str,
    ) -> pnpr::Result<(pnpr::UpsertOutcome, String)> {
        self.users.add_or_login(name, password).await
    }
    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        self.users.list_users().await
    }
    async fn credential(&self, name: &str) -> pnpr::Result<Option<pnpr_auth::AccountCredential>> {
        self.users.credential(name).await
    }
    async fn create_user(&self, name: &str, password: &str) -> pnpr::Result<pnpr::UserCreation> {
        self.users.create_user(name, password).await
    }
    async fn set_password(&self, id: &str, password: &str) -> pnpr::Result<pnpr::PasswordChange> {
        self.users.set_password(id, password).await
    }
    async fn set_password_if_current(
        &self,
        id: &str,
        password: &str,
        expected: &str,
    ) -> pnpr::Result<bool> {
        self.entered.notify_one();
        self.resume.notified().await;
        self.users.set_password_if_current(id, password, expected).await
    }
    async fn clear_password(&self, id: &str) -> pnpr::Result<pnpr::PasswordChange> {
        self.users.clear_password(id).await
    }
    async fn delete_user(&self, id: &str) -> pnpr::Result<pnpr::UserRemoval> {
        self.users.delete_user(id).await
    }
    async fn provision_user(&self, name: &str) -> pnpr::Result<String> {
        self.users.provision_user(name).await
    }
    async fn account_id(&self, name: &str) -> pnpr::Result<Option<String>> {
        self.users.account_id(name).await
    }
    async fn active_account_id(&self, name: &str) -> pnpr::Result<Option<String>> {
        self.users.active_account_id(name).await
    }
    async fn account_name(&self, id: &str) -> pnpr::Result<Option<String>> {
        self.users.account_name(id).await
    }
    async fn active_account_name(&self, id: &str) -> pnpr::Result<Option<String>> {
        self.users.active_account_name(id).await
    }
    async fn account_status(&self, id: &str) -> pnpr::Result<Option<pnpr_auth::AccountStatus>> {
        self.users.account_status(id).await
    }
    async fn apply_account_status(
        &self,
        id: &str,
        revision: u64,
        lifecycle: pnpr_auth::AccountLifecycle,
    ) -> pnpr::Result<pnpr_auth::AccountStatusOutcome> {
        self.users.apply_account_status(id, revision, lifecycle).await
    }
    fn revokes_tokens_in_status_transition(&self) -> bool {
        self.users.revokes_tokens_in_status_transition()
    }
    async fn rename_user(&self, id: &str, name: &str) -> pnpr::Result<()> {
        self.users.rename_user(id, name).await
    }
}

#[tokio::test]
async fn a_profile_update_cannot_overwrite_an_admin_reset_after_verification() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    users.create_user("example-user", "old-password").await.unwrap();
    let id = users
        .account_id("example-user")
        .await
        .unwrap()
        .unwrap();
    let delayed = Arc::new(PausedPasswordUpdate {
        users: Arc::clone(&users),
        entered: tokio::sync::Notify::new(),
        resume: tokio::sync::Notify::new(),
    });
    let tokens = Arc::new(TokenStore::in_memory());
    let token = tokens.issue(&id).await.unwrap();
    let auth = Arc::new(AuthState { users: Arc::clone(&delayed) as Arc<dyn UserBackend>, tokens });
    let app = account_router(load_config(&dir), (*auth).clone()).await;
    let request = tokio::spawn(async move {
        let update = json!({ "password": { "old": "old-password", "new": "attacker-password" } });
        send(&app, "POST", "/-/npm/v1/user", Some(&token), Some(update)).await
    });
    delayed.entered.notified().await;
    auth.change_password(&id, "admin-reset-password").await.unwrap();
    delayed.resume.notify_one();
    assert_eq!(request.await.unwrap().0, axum::http::StatusCode::UNAUTHORIZED);
    assert!(users.add_or_login("example-user", "admin-reset-password").await.is_ok());
    assert!(users.add_or_login("example-user", "attacker-password").await.is_err());
}

#[tokio::test]
async fn imported_sha_passwords_support_token_creation_and_profile_changes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("accounts.json");
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha1)
        .chain(b"imported-password")
        .result();
    let hash = format!("{{SHA}}{}", integrity.hashes.first().unwrap().digest);
    let hashes = std::collections::HashMap::from([("example-user".to_string(), hash.clone())]);
    UserStore::import_password_hashes(
        &path,
        hashes,
        pnpr_auth::PasswordImport {
            collisions: pnpr_auth::PasswordCollisions::Keep,
            mode: pnpr_auth::ImportMode::Persist,
        },
    )
    .unwrap();
    let users = Arc::new(UserStore::open_with_cost(path, pnpr::MaxUsers::Disabled, 4).unwrap());
    let id = users
        .account_id("example-user")
        .await
        .unwrap()
        .unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let token = tokens.issue(&id).await.unwrap();
    let auth = AuthState {
        users: Arc::clone(&users) as Arc<dyn UserBackend>,
        tokens: Arc::clone(&tokens) as Arc<dyn TokenBackend>,
    };
    let app = account_router(load_config(&dir), auth).await;
    let (status, _) = send(
        &app,
        "POST",
        "/-/npm/v1/tokens",
        Some(&token),
        Some(json!({ "password": "incorrect-password" })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    let (status, created) = send(
        &app,
        "POST",
        "/-/npm/v1/tokens",
        Some(&token),
        Some(json!({ "password": "imported-password" })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        users
            .credential("example-user")
            .await
            .unwrap()
            .as_ref()
            .and_then(pnpr_auth::AccountCredential::password_hash),
        Some(hash.as_str()),
    );
    let update = json!({ "password": { "old": "imported-password", "new": "changed-password" } });
    let (status, _) = send(&app, "POST", "/-/npm/v1/user", Some(&token), Some(update)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(users.add_or_login("example-user", "changed-password").await.is_ok());
    assert!(users.add_or_login("example-user", "imported-password").await.is_err());
    assert!(
        tokens
            .lookup(created["token"].as_str().unwrap())
            .await
            .unwrap()
            .is_none(),
    );
}

struct FailedPostUpdateLookup {
    users: Arc<UserStore>,
    has_updated: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl UserBackend for FailedPostUpdateLookup {
    async fn add_or_login(
        &self,
        name: &str,
        password: &str,
    ) -> pnpr::Result<(pnpr::UpsertOutcome, String)> {
        self.users.add_or_login(name, password).await
    }
    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        self.users.list_users().await
    }
    async fn credential(&self, name: &str) -> pnpr::Result<Option<pnpr_auth::AccountCredential>> {
        self.users.credential(name).await
    }
    async fn create_user(&self, name: &str, password: &str) -> pnpr::Result<pnpr::UserCreation> {
        let creation = self.users.create_user(name, password).await?;
        self.has_updated.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(creation)
    }
    async fn set_password(&self, id: &str, password: &str) -> pnpr::Result<pnpr::PasswordChange> {
        let changed = self.users.set_password(id, password).await?;
        self.has_updated.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(changed)
    }
    async fn clear_password(&self, id: &str) -> pnpr::Result<pnpr::PasswordChange> {
        self.users.clear_password(id).await
    }
    async fn delete_user(&self, id: &str) -> pnpr::Result<pnpr::UserRemoval> {
        self.users.delete_user(id).await
    }
    async fn provision_user(&self, name: &str) -> pnpr::Result<String> {
        self.users.provision_user(name).await
    }
    async fn account_id(&self, name: &str) -> pnpr::Result<Option<String>> {
        if self.has_updated.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(pnpr::RegistryError::StorageUnavailable {
                reason: "account id lookup failed".to_string(),
            });
        }
        self.users.account_id(name).await
    }
    async fn active_account_id(&self, name: &str) -> pnpr::Result<Option<String>> {
        if name == "example-user" && self.has_updated.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(pnpr::RegistryError::StorageUnavailable {
                reason: "account lookup failed".to_string(),
            });
        }
        self.users.active_account_id(name).await
    }
    async fn account_name(&self, id: &str) -> pnpr::Result<Option<String>> {
        if self.has_updated.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(pnpr::RegistryError::StorageUnavailable {
                reason: "account name lookup failed".to_string(),
            });
        }
        self.users.account_name(id).await
    }
    async fn active_account_name(&self, id: &str) -> pnpr::Result<Option<String>> {
        self.users.active_account_name(id).await
    }
    async fn account_status(&self, id: &str) -> pnpr::Result<Option<pnpr_auth::AccountStatus>> {
        self.users.account_status(id).await
    }
    async fn apply_account_status(
        &self,
        id: &str,
        revision: u64,
        lifecycle: pnpr_auth::AccountLifecycle,
    ) -> pnpr::Result<pnpr_auth::AccountStatusOutcome> {
        self.users.apply_account_status(id, revision, lifecycle).await
    }
    fn revokes_tokens_in_status_transition(&self) -> bool {
        self.users.revokes_tokens_in_status_transition()
    }
    async fn rename_user(&self, id: &str, name: &str) -> pnpr::Result<()> {
        self.users.rename_user(id, name).await
    }
}

#[tokio::test]
async fn admin_create_and_password_change_do_not_query_account_after_commit() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    let pnpr::UserCreation::Created(root_id) =
        users.create_user("root", "secret123").await.unwrap()
    else {
        panic!("expected root to be created");
    };
    let backend = Arc::new(FailedPostUpdateLookup {
        users: Arc::clone(&users),
        has_updated: std::sync::atomic::AtomicBool::new(false),
    });
    let tokens = Arc::new(TokenStore::in_memory());
    let token = tokens.issue(&root_id).await.unwrap();
    let auth = AuthState { users: Arc::clone(&backend) as Arc<dyn UserBackend>, tokens };
    let app = account_router(load_config(&dir), auth).await;
    let (status, created) = send(
        &app,
        "POST",
        "/-/pnpr/v0/admin/users",
        Some(&token),
        Some(json!({ "name": "example-user", "password": "old-password" })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    assert!(backend.account_id("example-user").await.is_err());
    assert!(backend.account_name(id).await.is_err());

    backend.has_updated.store(false, std::sync::atomic::Ordering::SeqCst);
    let (status, result) = send(
        &app,
        "PUT",
        &format!("/-/pnpr/v0/admin/users/{id}"),
        Some(&token),
        Some(json!({ "password": "new-password" })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(result, json!({ "id": id, "name": "example-user" }));
    assert!(backend.account_name(id).await.is_err());
    assert!(users.add_or_login("example-user", "new-password").await.is_ok());
}

#[tokio::test]
async fn a_custom_backend_reports_unsupported_password_changes() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    users.create_user("example-user", "old-password").await.unwrap();
    let id = users
        .account_id("example-user")
        .await
        .unwrap()
        .unwrap();
    let backend = FailedPostUpdateLookup {
        users: Arc::clone(&users),
        has_updated: std::sync::atomic::AtomicBool::new(false),
    };
    let tokens = Arc::new(TokenStore::in_memory());
    let token = tokens.issue(&id).await.unwrap();
    let auth = AuthState { users: Arc::new(backend), tokens };
    let app = account_router(load_config(&dir), auth).await;
    let update = json!({ "password": { "old": "old-password", "new": "new-password" } });
    let (status, result) = send(&app, "POST", "/-/npm/v1/user", Some(&token), Some(update)).await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("not supported"),
    );
    assert!(users.add_or_login("example-user", "old-password").await.is_ok());
}
