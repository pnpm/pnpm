//! Tokens for tests that drive a router built on an in-memory `AuthState`.

use pnpr::{AuthState, TokenBackend};
use pnpr_auth::LAST_USED_GRANULARITY_MILLIS;
use std::{sync::Arc, time::Duration};

/// A token for `username`, stored as an account first: pnpr honors only
/// tokens whose owner is an account.
pub(crate) async fn issue_token(auth: &AuthState, username: &str) -> String {
    let id = match auth.users.create_user(username, "password").await.unwrap() {
        pnpr::UserCreation::Created(id) => id,
        pnpr::UserCreation::NameTaken => auth.users
            .account_id(username)
            .await
            .unwrap()
            .unwrap(),
    };
    auth.tokens.issue(&id).await.unwrap()
}

/// Set the stored last-used time of `raw` back far enough that its next
/// use is due for a write, and return that time.
pub(crate) async fn age_last_use(tokens: &Arc<dyn TokenBackend>, raw: &str) -> u64 {
    let record = tokens
        .lookup_record(raw)
        .await
        .unwrap()
        .unwrap();
    let aged = record.last_used_at - 2 * LAST_USED_GRANULARITY_MILLIS;
    tokens.touch(raw, aged).await.unwrap();
    aged
}

/// Wait until a use recorded off the request path has advanced the stored
/// last-used time of `raw` past `previous`.
pub(crate) async fn wait_for_last_use_after(
    tokens: &Arc<dyn TokenBackend>,
    raw: &str,
    previous: u64,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while tokens
            .lookup_record(raw)
            .await
            .unwrap()
            .unwrap()
            .last_used_at
            <= previous
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the last-used write lands");
}

/// Explicit account records let policy fixtures refer to known immutable IDs.
pub(crate) fn seeded_auth(config: &pnpr::Config, accounts: &[(&str, &str)]) -> AuthState {
    let path = config.storage.hosted_dir.join("test-accounts.json");
    std::fs::create_dir_all(&config.storage.hosted_dir).unwrap();
    let mut records: serde_json::Map<String, serde_json::Value> = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => serde_json::Map::default(),
        Err(err) => panic!("read fixture accounts: {err}"),
    };
    for (name, password) in accounts {
        records
            .entry((*name).to_string())
            .or_insert_with(|| {
                serde_json::json!({
                    "id": name,
                    "status": "active",
                    "credential_kind": "bcrypt",
                    "password_hash": bcrypt::hash(password, 4).unwrap(),
                    "created_at": 1000,
                })
            });
    }
    std::fs::write(&path, serde_json::to_vec(&records).unwrap()).unwrap();
    let users =
        Arc::new(pnpr::UserStore::open(path, config.identity.auth.users.max_users).unwrap());
    let tokens = pnpr::OwnedTokens::new(pnpr::TokenStore::in_memory(), Arc::clone(&users));
    AuthState { users, tokens: Arc::new(tokens) }
}

pub(crate) async fn account_id(
    app: &axum::Router,
    name: &str,
    token: Option<&str>,
) -> Option<String> {
    use tower::ServiceExt as _;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/-/user/{name}"))
                .header("authorization", format!("Bearer {}", token.unwrap_or("invalid")))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    if !response.status().is_success() {
        return None;
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"].as_str().map(str::to_string)
}
