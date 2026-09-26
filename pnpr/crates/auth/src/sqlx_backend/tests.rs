use super::{
    super::{MAX_USERNAME_CHARS, TokenBackend, TokenRecord, UpsertOutcome, UserBackend},
    AuthSqlBackend, InsertUser, SqlAuth, StoredUser,
};
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
        Ok(InsertUser::Existing(self.user.clone()))
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        Ok(())
    }

    async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
        Ok(None)
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
}

#[async_trait]
impl AuthSqlBackend for SlowLookupBackend {
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

    async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
        tokio::time::sleep(Duration::from_mins(1)).await;
        Ok(None)
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
}

#[async_trait]
impl AuthSqlBackend for SlowWriteBackend {
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
        Ok(InsertUser::Created)
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(())
    }

    async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
        Ok(None)
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
}

#[async_trait]
impl AuthSqlBackend for CountingLookupBackend {
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
        Ok(InsertUser::Created)
    }

    async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
        Ok(())
    }

    async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
        Ok(None)
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
}

#[async_trait]
impl AuthSqlBackend for CappedBackend {
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

    async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
        Ok(None)
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
}

#[tokio::test]
async fn add_or_login_propagates_corrupt_hash_errors() {
    let auth = SqlAuth::new(
        CanonicalBackend {
            user: StoredUser {
                username: "Alice".to_string(),
                bcrypt_hash: "not-a-bcrypt-hash".to_string(),
            },
        },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );

    let err = auth.add_or_login("alice", "secret").await.unwrap_err();

    assert!(matches!(err, RegistryError::Bcrypt(_)), "got {err:?}");
}

#[tokio::test]
async fn add_or_login_returns_the_stored_username_for_existing_users() {
    let bcrypt_hash = bcrypt::hash("secret", 4).unwrap();
    let auth = SqlAuth::new(
        CanonicalBackend { user: StoredUser { username: "Alice".to_string(), bcrypt_hash } },
        MaxUsers::Unlimited,
        Duration::from_secs(30),
    );

    let outcome = auth.add_or_login("alice", "secret").await.unwrap();

    assert!(matches!(outcome, (UpsertOutcome::LoggedIn, _)));
    assert_eq!(outcome.1, "Alice");
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
        let err = auth.add_or_login(username, "secret").await.unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
    let err = auth.add_or_login(&overlong, "secret").await.unwrap_err();
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
        let err = auth.add_or_login(username, "secret").await.unwrap_err();
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
