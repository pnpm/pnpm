use super::{
    Body, Config, Ipv4Addr, Request, ServiceExt, SocketAddr, SocketAddrV4, StatusCode, TempDir,
    body_json, header, json, router,
};

fn config_with_providers(storage: &TempDir) -> Config {
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        storage.path().to_path_buf(),
    );
    // OIDC redirect URIs must be HTTPS.
    config.http.public_url = "https://registry.example".to_string();
    config.identity.auth.oidc = serde_json::from_value(json!([
        {
            "name": "company", "issuer": "https://login.example", "audience": "pnpr",
            "login": { "users": [{ "subject": "alice", "username": "alice" }] }
        },
        {
            "name": "github", "issuer": "https://token.actions.githubusercontent.com",
            "audience": "pnpr",
            "workloads": [{
                "identity": { "subject": "repo:org/repo:ref:refs/heads/main", "username": "ci" },
                "registry": "local", "packages": ["foo"]
            }]
        }
    ]))
    .unwrap();
    config
}

#[tokio::test]
async fn lists_the_providers_that_offer_browser_sign_in() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_providers(&storage));

    let response = app
        .oneshot(
            Request::get("/-/pnpr/v0/sign-in")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(body_json(response.into_body()).await, json!({ "oidc": [{ "name": "company" }] }));
}

#[tokio::test]
async fn an_unknown_handoff_code_is_refused() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_providers(&storage));

    let response = app
        .oneshot(
            Request::post("/-/oidc/handoff")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "code": "unknown" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_in_returns_only_to_the_ui() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_providers(&storage));

    let response = app
        .oneshot(
            Request::get("/-/oidc/company/login?return=https://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
