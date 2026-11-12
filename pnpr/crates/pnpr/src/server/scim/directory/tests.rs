use super::ScimState;
use pnpr_auth::AuthState;
use pnpr_config::{Config, ScimConfig};
use pnpr_fixtures::MockConfigPresets;
use pnpr_storage::metadata::ScimStatus;
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

#[tokio::test]
async fn deactivation_holds_the_binding_lock_until_revocation_finishes() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.identity.auth.scim =
        Some(ScimConfig { token: "scim-secret-0123456789abcdef0123456789".to_string() });
    let (state, _) = crate::server::state_and_router(config, AuthState::in_memory(), None).unwrap();
    let directory = Arc::new(ScimState::default());
    let mut status = ScimStatus {
        user_id: "example-account".to_string(),
        external_id: Some("subject-one".to_string()),
        active: true,
        deprovisions: 0,
        generation: 1,
    };
    directory.apply_statuses(&state, &[status.clone()]);
    let tracker = directory.tracker.lock().unwrap();
    status.active = false;
    status.deprovisions = 1;
    status.generation = 2;
    let updating = Arc::clone(&directory);
    let worker = thread::spawn(move || updating.apply_statuses(&state, &[status]));
    let deadline = Instant::now() + Duration::from_secs(5);
    let inactive = loop {
        if directory.active_by_id
            .read()
            .unwrap()
            .get("example-account")
            == Some(&false)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(1));
    };
    let binding_locked = directory.subjects.try_read().is_err();
    drop(tracker);
    worker.join().unwrap();
    assert!(inactive, "deactivation must reach the revocation phase");
    assert!(binding_locked, "subject resolution must wait until revocation completes");
    assert!(directory.subjects.try_read().is_ok());
}

#[tokio::test]
async fn superseded_deactivation_preserves_post_reactivation_tokens_and_directory_state() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.identity.auth.scim =
        Some(ScimConfig { token: "scim-secret-0123456789abcdef0123456789".to_string() });
    let auth = AuthState::in_memory();
    let (state, _) = crate::server::state_and_router(config, auth.clone(), None).unwrap();

    let active_user = super::ScimUser {
        username: "example-user".to_string(),
        active: true,
        attributes: serde_json::Map::new(),
    };
    let (id, _) = super::create_user(&state, "example-user", &active_user).await.unwrap();

    let pnpr_storage::metadata::ScimUserWrite::Written(stale_deactivation) = state
        .inner
        .storage
        .replace_scim_user(
            Some(1),
            pnpr_storage::metadata::NewScimUser {
                user_id: id.clone(),
                active: false,
                attributes: serde_json::Map::new(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected scim deactivation write to succeed");
    };

    super::replace_user(&state, &id, &active_user).await.unwrap();
    auth.users.set_password(&id, "secret123").await.unwrap();
    let fresh_token = auth.tokens.issue(&id).await.unwrap();

    super::writes::finish_scim_write(&state, stale_deactivation).await.unwrap();

    assert_eq!(
        auth.users.account_status(&id).await.unwrap(),
        Some(pnpr_auth::AccountStatus::Active),
    );
    assert_eq!(auth.tokens.lookup(&fresh_token).await.unwrap(), Some(id.clone()));
    assert!(
        auth.users
            .credential("example-user")
            .await
            .unwrap()
            .and_then(|cred| cred.password_hash().map(str::to_string))
            .is_some(),
    );
    assert!(!super::is_deprovisioned(&state, &id).unwrap());
}

struct CountingStatusBackend {
    inner: Arc<pnpr_auth::UserStore>,
    revisions_calls: std::sync::atomic::AtomicUsize,
    apply_calls: std::sync::atomic::AtomicUsize,
    fail_post_reconcile_name: std::sync::atomic::AtomicBool,
}

impl CountingStatusBackend {
    fn new(inner: Arc<pnpr_auth::UserStore>) -> Self {
        Self {
            inner,
            revisions_calls: std::sync::atomic::AtomicUsize::new(0),
            apply_calls: std::sync::atomic::AtomicUsize::new(0),
            fail_post_reconcile_name: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait::async_trait]
impl pnpr_auth::UserBackend for CountingStatusBackend {
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> pnpr_error::Result<(pnpr_auth::UpsertOutcome, String)> {
        self.inner.add_or_login(username, password).await
    }
    async fn list_users(&self) -> pnpr_error::Result<Vec<String>> {
        self.inner.list_users().await
    }
    async fn credential(
        &self,
        username: &str,
    ) -> pnpr_error::Result<Option<pnpr_auth::AccountCredential>> {
        self.inner.credential(username).await
    }
    async fn create_user(
        &self,
        username: &str,
        password: &str,
    ) -> pnpr_error::Result<pnpr_auth::UserCreation> {
        self.inner.create_user(username, password).await
    }
    async fn set_password(
        &self,
        user_id: &str,
        password: &str,
    ) -> pnpr_error::Result<pnpr_auth::PasswordChange> {
        self.inner.set_password(user_id, password).await
    }
    async fn clear_password(&self, user_id: &str) -> pnpr_error::Result<pnpr_auth::PasswordChange> {
        self.inner.clear_password(user_id).await
    }
    async fn delete_user(&self, user_id: &str) -> pnpr_error::Result<pnpr_auth::UserRemoval> {
        self.inner.delete_user(user_id).await
    }
    async fn provision_user(&self, username: &str) -> pnpr_error::Result<String> {
        self.inner.provision_user(username).await
    }
    async fn account_id(&self, username: &str) -> pnpr_error::Result<Option<String>> {
        self.inner.account_id(username).await
    }
    async fn active_account_id(&self, username: &str) -> pnpr_error::Result<Option<String>> {
        self.inner.active_account_id(username).await
    }
    async fn account_name(&self, user_id: &str) -> pnpr_error::Result<Option<String>> {
        if self.fail_post_reconcile_name.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(pnpr_error::RegistryError::StorageUnavailable {
                reason: "simulated post-reconcile name lookup failure".to_string(),
            });
        }
        self.inner.account_name(user_id).await
    }
    async fn active_account_name(&self, user_id: &str) -> pnpr_error::Result<Option<String>> {
        self.inner.active_account_name(user_id).await
    }
    async fn account_status(
        &self,
        user_id: &str,
    ) -> pnpr_error::Result<Option<pnpr_auth::AccountStatus>> {
        self.inner.account_status(user_id).await
    }
    async fn status_revisions(
        &self,
        user_ids: &[String],
    ) -> pnpr_error::Result<std::collections::HashMap<String, pnpr_auth::AccountStatusRevision>>
    {
        self.revisions_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.status_revisions(user_ids).await
    }
    async fn apply_account_status(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: pnpr_auth::AccountLifecycle,
    ) -> pnpr_error::Result<pnpr_auth::AccountStatusOutcome> {
        self.apply_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.apply_account_status(user_id, revision, lifecycle).await
    }
    fn revokes_tokens_in_status_transition(&self) -> bool {
        self.inner.revokes_tokens_in_status_transition()
    }
    async fn account_login(
        &self,
        user_id: &str,
    ) -> pnpr_error::Result<Option<pnpr_auth::AccountLogin>> {
        self.inner.account_login(user_id).await
    }
    async fn rename_user(&self, user_id: &str, username: &str) -> pnpr_error::Result<()> {
        self.inner.rename_user(user_id, username).await
    }
    async fn rename_user_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> pnpr_error::Result<pnpr_auth::UserRename> {
        self.inner.rename_user_if_current(user_id, expected_revision, username).await
    }
}

#[tokio::test]
async fn directory_reload_skips_writes_for_already_reconciled_accounts() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.identity.auth.scim =
        Some(ScimConfig { token: "scim-secret-0123456789abcdef0123456789".to_string() });
    let store = Arc::new(pnpr_auth::UserStore::in_memory());
    let counting = Arc::new(CountingStatusBackend::new(Arc::clone(&store)));
    let tokens = Arc::new(pnpr_auth::OwnedTokens::new(
        pnpr_auth::TokenStore::in_memory(),
        Arc::clone(&store),
    ));
    let auth =
        AuthState { users: Arc::clone(&counting) as Arc<dyn pnpr_auth::UserBackend>, tokens };
    let (state, _) = crate::server::state_and_router(config, auth, None).unwrap();

    let active_spec = super::ScimUser {
        username: "active-member".to_string(),
        active: true,
        attributes: serde_json::Map::new(),
    };
    let (active_id, _) = super::create_user(&state, "active-member", &active_spec)
        .await
        .unwrap();

    let inactive_spec = super::ScimUser {
        username: "inactive-member".to_string(),
        active: false,
        attributes: serde_json::Map::new(),
    };
    let (_inactive_id, _) = super::create_user(&state, "inactive-member", &inactive_spec)
        .await
        .unwrap();

    let removed_spec = super::ScimUser {
        username: "removed-member".to_string(),
        active: true,
        attributes: serde_json::Map::new(),
    };
    let (removed_id, _) = super::create_user(&state, "removed-member", &removed_spec)
        .await
        .unwrap();
    super::delete_user(&state, &removed_id).await.unwrap();

    counting.revisions_calls.store(0, std::sync::atomic::Ordering::SeqCst);
    counting.apply_calls.store(0, std::sync::atomic::Ordering::SeqCst);

    let cold_directory = ScimState::default();
    assert!(super::reload_directory(&state, &cold_directory).await);
    assert_eq!(counting.revisions_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(counting.apply_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let current = state.inner.storage
        .read_scim_user(&active_id)
        .await
        .unwrap()
        .unwrap();
    state.inner.storage
        .replace_scim_user(
            Some(current.generation),
            pnpr_storage::metadata::NewScimUser {
                user_id: active_id.clone(),
                active: false,
                attributes: serde_json::Map::new(),
            },
        )
        .await
        .unwrap();

    assert!(super::reload_directory(&state, &cold_directory).await);
    assert_eq!(counting.apply_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_post_reconcile_name_lookup_does_not_mark_directory_unreadable() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.identity.auth.scim =
        Some(ScimConfig { token: "scim-secret-0123456789abcdef0123456789".to_string() });
    let store = Arc::new(pnpr_auth::UserStore::in_memory());
    let counting = Arc::new(CountingStatusBackend::new(Arc::clone(&store)));
    let tokens = Arc::new(pnpr_auth::OwnedTokens::new(
        pnpr_auth::TokenStore::in_memory(),
        Arc::clone(&store),
    ));
    let auth =
        AuthState { users: Arc::clone(&counting) as Arc<dyn pnpr_auth::UserBackend>, tokens };
    let (state, _) = crate::server::state_and_router(config, auth, None).unwrap();

    let active_spec = super::ScimUser {
        username: "member-one".to_string(),
        active: true,
        attributes: serde_json::Map::new(),
    };
    let (member_one, _) = super::create_user(&state, "member-one", &active_spec)
        .await
        .unwrap();
    let other_spec = super::ScimUser {
        username: "member-two".to_string(),
        active: true,
        attributes: serde_json::Map::new(),
    };
    let (member_two, _) = super::create_user(&state, "member-two", &other_spec)
        .await
        .unwrap();

    let pnpr_storage::metadata::ScimUserWrite::Written(deactivation) = state
        .inner
        .storage
        .replace_scim_user(
            Some(1),
            pnpr_storage::metadata::NewScimUser {
                user_id: member_one.clone(),
                active: false,
                attributes: serde_json::Map::new(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected scim deactivation write to succeed");
    };

    counting.fail_post_reconcile_name.store(true, std::sync::atomic::Ordering::SeqCst);
    let err = super::writes::finish_scim_write(&state, deactivation).await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::SERVICE_UNAVAILABLE);

    assert!(super::is_deprovisioned(&state, &member_one).unwrap());
    assert!(!super::is_deprovisioned(&state, &member_two).unwrap());
}

#[tokio::test]
async fn directory_reload_retries_failed_token_revocation_until_reconciled() {
    let temp = tempfile::TempDir::new().unwrap();
    let users_path = temp.path().join("users.json");
    let tokens_path = temp.path().join("tokens.db");
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.identity.auth.scim =
        Some(ScimConfig { token: "scim-secret-0123456789abcdef0123456789".to_string() });
    let store = Arc::new(
        pnpr_auth::UserStore::open_with_cost(users_path, pnpr_config::MaxUsers::Unlimited, 4)
            .unwrap(),
    );
    let tokens = Arc::new(pnpr_auth::OwnedTokens::new(
        pnpr_auth::TokenStore::open(tokens_path.clone()).unwrap(),
        Arc::clone(&store),
    ));
    let auth = AuthState {
        users: Arc::clone(&store) as Arc<dyn pnpr_auth::UserBackend>,
        tokens: Arc::clone(&tokens) as Arc<dyn pnpr_auth::TokenBackend>,
    };
    let (state, _) = crate::server::state_and_router(config, auth.clone(), None).unwrap();

    let active_spec = super::ScimUser {
        username: "member-one".to_string(),
        active: true,
        attributes: serde_json::Map::new(),
    };
    let (member_id, _) = super::create_user(&state, "member-one", &active_spec)
        .await
        .unwrap();
    let token = auth.tokens.issue(&member_id).await.unwrap();
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), Some(member_id.clone()));

    let current = state.inner.storage
        .read_scim_user(&member_id)
        .await
        .unwrap()
        .unwrap();
    state.inner.storage
        .replace_scim_user(
            Some(current.generation),
            pnpr_storage::metadata::NewScimUser {
                user_id: member_id.clone(),
                active: false,
                attributes: serde_json::Map::new(),
            },
        )
        .await
        .unwrap();

    let fault = rusqlite::Connection::open(&tokens_path).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER fail_revocation BEFORE DELETE ON tokens
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

    let directory = ScimState::default();
    assert!(!super::reload_directory(&state, &directory).await);
    assert_eq!(
        auth.users.account_status(&member_id).await.unwrap(),
        Some(pnpr_auth::AccountStatus::Deactivated),
    );
    assert!(!super::reload_directory(&state, &directory).await);

    fault.execute_batch("DROP TRIGGER fail_revocation;").unwrap();

    assert!(super::reload_directory(&state, &directory).await);
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);

    super::replace_user(&state, &member_id, &active_spec).await.unwrap();
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);
}
