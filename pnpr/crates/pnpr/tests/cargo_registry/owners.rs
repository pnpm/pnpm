use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, cargo_config,
    crate_archive, header, json, metadata, publish_body, publish_request, router_with_auth,
};
use crate::tokens::issue_token;
use axum::Router;

async fn owner_request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    users: &[&str],
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::AUTHORIZATION, token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "users": users }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    (status, body)
}

async fn logins(app: &Router, path: &str, token: &str) -> Vec<String> {
    let response = app
        .clone()
        .oneshot(
            Request::get(path)
                .header(header::AUTHORIZATION, token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    body["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| {
            user["login"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

async fn publish(app: &Router, token: &str, version: &str) -> StatusCode {
    let body = publish_body(&metadata("demo", version), &crate_archive("demo", version));
    app.clone()
        .oneshot(publish_request(Some(token), body))
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn cargo_owner_lists_adds_and_removes_owners() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let alice = issue_token(&auth, "alice").await;
    let bob = issue_token(&auth, "bob").await;
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let owners = "/cargo/api/v1/crates/demo/owners";
    assert_eq!(publish(&app, &alice, "0.1.0").await, StatusCode::OK);
    assert_eq!(logins(&app, owners, &bob).await, ["alice"]);

    let (status, body) = owner_request(&app, "PUT", owners, &bob, &["bob"]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["errors"][0]["detail"].is_string(), "{body}");

    let (status, body) = owner_request(&app, "PUT", owners, &alice, &["bob"]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert!(body["msg"].is_string(), "{body}");
    let named = "/cargo/~crates/api/v1/crates/demo/owners";
    assert_eq!(logins(&app, named, &alice).await, ["alice", "bob"]);
    assert_eq!(publish(&app, &bob, "0.2.0").await, StatusCode::OK);

    let (status, body) = owner_request(&app, "DELETE", owners, &bob, &["alice"]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(logins(&app, owners, &bob).await, ["bob"]);
    assert_eq!(publish(&app, &alice, "0.3.0").await, StatusCode::FORBIDDEN);
    let (status, _) = owner_request(&app, "PUT", owners, &alice, &["alice"]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn cargo_owner_rejects_an_unknown_user_and_removing_the_last_owner() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let alice = issue_token(&auth, "alice").await;
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let owners = "/cargo/api/v1/crates/demo/owners";
    assert_eq!(publish(&app, &alice, "0.1.0").await, StatusCode::OK);

    let (status, body) = owner_request(&app, "PUT", owners, &alice, &["nobody"]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("unknown user"),
        "{body}",
    );
    let (status, body) = owner_request(&app, "DELETE", owners, &alice, &["alice"]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("at least one owner"),
        "{body}",
    );
    assert_eq!(logins(&app, owners, &alice).await, ["alice"]);
}

#[tokio::test]
async fn cargo_owner_of_an_unpublished_crate_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let alice = issue_token(&auth, "alice").await;
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    for method in ["GET", "PUT", "DELETE"] {
        let (status, body) =
            owner_request(&app, method, "/cargo/api/v1/crates/demo/owners", &alice, &["alice"])
                .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method}");
        assert!(body["errors"][0]["detail"].is_string(), "{method} {body}");
    }
}

#[tokio::test]
async fn cargo_owner_changes_need_a_signed_in_caller() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let alice = issue_token(&auth, "alice").await;
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    assert_eq!(publish(&app, &alice, "0.1.0").await, StatusCode::OK);
    for method in ["PUT", "DELETE"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/cargo/api/v1/crates/demo/owners")
                    .body(Body::from(r#"{"users":["alice"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{method}");
    }
}
