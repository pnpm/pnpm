use super::{
    super::{PasswordChange, TokenBackend, TokenRecord, UpsertOutcome, UserBackend, UserRemoval},
    AuthSqlBackend, InsertUser, SqlAuth, StoredUser,
};
use crate::validation::MAX_USERNAME_CHARS;
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::{RegistryError, Result};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

struct CanonicalBackend {
    confusable_insert: bool,
    user: StoredUser,
}

struct SlowLookupBackend;

struct SlowWriteBackend;

struct CountingLookupBackend {
    stored_user_calls: Arc<AtomicU64>,
}

struct CappedBackend {
    reconcile_calls: Arc<AtomicU64>,
}

#[async_trait]
impl AuthSqlBackend for CanonicalBackend {
    async fn account_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self.stored_user(id).await?.map(|account| account.username))
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> Result<()> {
        unreachable!()
    }

    async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
        Ok(Some(self.user.clone()))
    }

    async fn user_count(&self) -> Result<u64> {
        Ok(1)
    }

    async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
        Ok(false)
    }

    async fn insert_user(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
        _max_users: MaxUsers,
    ) -> Result<InsertUser> {
        if self.confusable_insert {
            return Ok(InsertUser::Confusable);
        }
        Ok(InsertUser::Existing(self.user.clone()))
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        Ok(())
    }

    async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }

    async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn delete_token(&self, _token_hash: &str) -> Result<()> {
        Ok(())
    }

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn update_password_hash(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
    ) -> Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }
}

#[async_trait]
impl AuthSqlBackend for SlowLookupBackend {
    async fn account_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self.stored_user(id).await?.map(|account| account.username))
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> Result<()> {
        unreachable!()
    }

    async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
        Ok(None)
    }

    async fn user_count(&self) -> Result<u64> {
        Ok(0)
    }

    async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
        Ok(false)
    }

    async fn insert_user(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
        _max_users: MaxUsers,
    ) -> Result<InsertUser> {
        Ok(InsertUser::CapReached)
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        Ok(())
    }

    async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
        tokio::time::sleep(Duration::from_mins(1)).await;
        Ok(None)
    }

    async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn delete_token(&self, _token_hash: &str) -> Result<()> {
        Ok(())
    }

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn update_password_hash(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
    ) -> Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }
}

#[async_trait]
impl AuthSqlBackend for SlowWriteBackend {
    async fn account_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self.stored_user(id).await?.map(|account| account.username))
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> Result<()> {
        unreachable!()
    }

    async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
        Ok(None)
    }

    async fn user_count(&self) -> Result<u64> {
        Ok(0)
    }

    async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
        Ok(false)
    }

    async fn insert_user(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
        _max_users: MaxUsers,
    ) -> Result<InsertUser> {
        Ok(InsertUser::Created("account-id".to_string()))
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(())
    }

    async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }

    async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn delete_token(&self, _token_hash: &str) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(())
    }

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn update_password_hash(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
    ) -> Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }
}

#[async_trait]
impl AuthSqlBackend for CountingLookupBackend {
    async fn account_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self.stored_user(id).await?.map(|account| account.username))
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> Result<()> {
        unreachable!()
    }

    async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
        self.stored_user_calls.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }

    async fn user_count(&self) -> Result<u64> {
        Ok(0)
    }

    async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
        Ok(false)
    }

    async fn insert_user(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
        _max_users: MaxUsers,
    ) -> Result<InsertUser> {
        Ok(InsertUser::Created("account-id".to_string()))
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        Ok(())
    }

    async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }

    async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn delete_token(&self, _token_hash: &str) -> Result<()> {
        Ok(())
    }

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn update_password_hash(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
    ) -> Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }
}

#[async_trait]
impl AuthSqlBackend for CappedBackend {
    async fn account_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self.stored_user(id).await?.map(|account| account.username))
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> Result<()> {
        unreachable!()
    }

    async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
        Ok(None)
    }

    async fn user_count(&self) -> Result<u64> {
        Ok(1)
    }

    async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
        self.reconcile_calls.fetch_add(1, Ordering::SeqCst);
        Ok(false)
    }

    async fn insert_user(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
        _max_users: MaxUsers,
    ) -> Result<InsertUser> {
        panic!("capped precheck should reject before insert_user")
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        Ok(())
    }

    async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }

    async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn delete_token(&self, _token_hash: &str) -> Result<()> {
        Ok(())
    }

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn update_password_hash(
        &self,
        _username: &str,
        _bcrypt_hash: &str,
    ) -> Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }
}

#[tokio::test]
async fn add_or_login_propagates_corrupt_hash_errors() {
    let auth = SqlAuth::new(
        CanonicalBackend {
            confusable_insert: false,
            user: StoredUser {
                id: "account-id".to_string(),
                username: "Alice".to_string(),
                bcrypt_hash: "not-a-bcrypt-hash".to_string(),
            },
        },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );

    let err = auth.add_or_login("alice", "secret123").await.unwrap_err();

    assert!(matches!(err, RegistryError::Bcrypt(_)), "got {err:?}");
}

#[tokio::test]
async fn add_or_login_returns_the_immutable_id_for_existing_users() {
    let bcrypt_hash = bcrypt::hash("secret123", 4).unwrap();
    let auth = SqlAuth::new(
        CanonicalBackend {
            confusable_insert: false,
            user: StoredUser {
                id: "account-id".to_string(),
                username: "Alice".to_string(),
                bcrypt_hash,
            },
        },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );

    let outcome = auth.add_or_login("alice", "secret123").await.unwrap();

    assert!(matches!(outcome, (UpsertOutcome::LoggedIn, _)));
    assert_eq!(outcome.1, "account-id");
}

#[tokio::test]
async fn add_or_login_rejects_invalid_usernames_without_db_lookup() {
    let stored_user_calls = Arc::new(AtomicU64::new(0));
    let auth = SqlAuth::new(
        CountingLookupBackend { stored_user_calls: Arc::clone(&stored_user_calls) },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );
    let overlong = "a".repeat(MAX_USERNAME_CHARS + 1);

    for username in ["", " alice", "alice ", "#alice", "alice:admin", "alice\nadmin"] {
        let err = auth.add_or_login(username, "secret123").await.unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
    let err = auth.add_or_login(&overlong, "secret123").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(stored_user_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn add_or_login_rate_limits_capped_reconciliation() {
    let reconcile_calls = Arc::new(AtomicU64::new(0));
    let auth = SqlAuth::new(
        CappedBackend { reconcile_calls: Arc::clone(&reconcile_calls) },
        MaxUsers::Limited(1),
        Duration::from_secs(30),
    );

    for username in ["alice", "bob", "carol"] {
        let err = auth.add_or_login(username, "secret123").await.unwrap_err();
        assert!(matches!(err, RegistryError::TooManyUsers { max: 1 }));
    }

    assert_eq!(reconcile_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn token_lookup_times_out_when_the_backend_stalls() {
    let auth = SqlAuth::new(SlowLookupBackend, MaxUsers::Unlimited, Duration::from_millis(1));

    let err = auth.lookup("token").await.unwrap_err();

    assert!(matches!(err, RegistryError::AuthDatabaseTimeout));
}

#[tokio::test]
async fn token_issue_waits_for_slow_backend_write() {
    let auth = SqlAuth::new(SlowWriteBackend, MaxUsers::Unlimited, Duration::from_millis(1));

    let token = auth.issue("alice").await.unwrap();

    assert!(!token.is_empty());
}

#[tokio::test]
async fn provisioning_confusable_names_returns_the_validation_error() {
    let auth = SqlAuth::new(
        CanonicalBackend {
            confusable_insert: true,
            user: StoredUser {
                id: "account-id".to_string(),
                username: "modern".to_string(),
                bcrypt_hash: crate::NO_PASSWORD_HASH.to_string(),
            },
        },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );
    let error = auth.provision_user("rnodern").await.unwrap_err();
    assert!(matches!(error, RegistryError::BadRequest { .. }));
    assert_eq!(error.to_string(), crate::validation::confusable_username().to_string());
}

#[tokio::test]
async fn password_policy_rejects_existing_short_credentials() {
    let auth = SqlAuth::new(
        CanonicalBackend {
            confusable_insert: false,
            user: StoredUser {
                id: "account-id".to_string(),
                username: "member".to_string(),
                bcrypt_hash: bcrypt::hash("short", 4).unwrap(),
            },
        },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );
    let error = auth.add_or_login("member", "short").await.unwrap_err();
    assert!(matches!(error, RegistryError::BadRequest { .. }));
}

pub(super) fn saturated_pool_settings(variable: &str) -> pnpr_config::SqlBackendSettings {
    pnpr_config::SqlBackendSettings {
        url: std::env::var(variable).expect("database test URL"),
        max_connections: Some(1),
        timeout: Duration::from_millis(250),
        startup_timeout: Duration::from_secs(5),
    }
}

pub(super) async fn assert_saturated_auth_writes_timeout(database: impl AuthSqlBackend) {
    let insert = tokio::time::timeout(
        Duration::from_secs(2),
        database.insert_user("example", "unused", MaxUsers::Unlimited),
    )
    .await
    .expect("user write waited for the startup deadline")
    .map(|_| ());
    assert!(matches!(insert, Err(RegistryError::Sqlx(sqlx::Error::PoolTimedOut))), "{insert:?}");
    let delete = tokio::time::timeout(Duration::from_secs(2), database.delete_token("missing"))
        .await
        .expect("token write waited for the startup deadline");
    assert!(matches!(delete, Err(RegistryError::Sqlx(sqlx::Error::PoolTimedOut))), "{delete:?}");
}

pub(super) async fn assert_network_auth_roundtrip(auth: impl UserBackend + TokenBackend) {
    let id = auth.provision_user("example").await.unwrap();
    assert!(auth.provision_user("example").await.is_err());
    auth.rename_user(&id, "renamed").await.unwrap();
    assert_eq!(
        auth.account_name(&id).await
            .unwrap()
            .as_deref(),
        Some("renamed"),
    );
    assert_eq!(
        auth.accounts_by_id(std::slice::from_ref(&id)).await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(auth.list_users().await.unwrap(), ["renamed"]);
    assert_eq!(
        auth.list_accounts().await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(auth.account_id("renamed").await.unwrap(), Some(id.clone()));
    let token = auth.issue(&id).await.unwrap();
    assert_eq!(auth.lookup(&token).await.unwrap(), Some(id.clone()));
    auth.delete_user(&id).await.unwrap();
    assert!(
        auth.lookup(&token).await
            .unwrap()
            .is_none(),
    );
}

pub(super) fn test_database_name(directory: &tempfile::TempDir) -> String {
    format!(
        "auth_{}",
        directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(['.', '-'], "")
            .to_ascii_lowercase(),
    )
}
