//! Tokens for tests that drive a router built on an in-memory `AuthState`.

use pnpr::{AuthState, TokenBackend};
use pnpr_auth::LAST_USED_GRANULARITY_SECS;
use std::{sync::Arc, time::Duration};

/// A token for `username`, stored as an account first: pnpr honors only
/// tokens whose owner is an account.
pub(crate) async fn issue_token(auth: &AuthState, username: &str) -> String {
    auth.users.create_user(username, "password").await.unwrap();
    let id = auth.users
        .account_id(username)
        .await
        .unwrap()
        .unwrap();
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
    let aged = record.last_used_at - 2 * LAST_USED_GRANULARITY_SECS;
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
                    "password_hash": bcrypt::hash(password, 4).unwrap(),
                })
            });
    }
    std::fs::write(&path, serde_json::to_vec(&records).unwrap()).unwrap();
    let users =
        Arc::new(pnpr::UserStore::open(path, config.identity.auth.users.max_users).unwrap());
    let tokens = pnpr::OwnedTokens::new(pnpr::TokenStore::in_memory(), Arc::clone(&users));
    AuthState { users, tokens: Arc::new(tokens) }
}

pub(crate) async fn account_id(app: &axum::Router, name: &str) -> Option<String> {
    use tower::ServiceExt as _;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/-/user/{name}"))
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

pub(crate) async fn admin_path(app: &axum::Router, method: &str, path: &str) -> String {
    let prefix = "/-/pnpr/v0/admin/users/";
    let Some(rest) = path.strip_prefix(prefix) else {
        return path.to_string();
    };
    if method == "PUT" {
        return path.to_string();
    }
    let (name, tail) = rest
        .split_once('/')
        .map_or((rest, ""), |(name, tail)| (name, tail));
    let Some(id) = account_id(app, name).await else {
        return path.to_string();
    };
    if tail.is_empty() { format!("{prefix}{id}") } else { format!("{prefix}{id}/{tail}") }
}
