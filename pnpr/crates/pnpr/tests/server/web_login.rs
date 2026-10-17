use super::{
    Body, Config, Ipv4Addr, Request, ServiceExt, SocketAddr, SocketAddrV4, StatusCode, TempDir,
    body_bytes, body_json, header, json, router,
};
use pnpr_fixtures::MockConfigPresets;

fn config_without_oidc(storage: &TempDir) -> Config {
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        storage.path().to_path_buf(),
    );
    config.http.public_url = "https://registry.example".to_string();
    config
}

fn config_with_single_login_provider(storage: &TempDir) -> Config {
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        storage.path().to_path_buf(),
    );
    config.http.public_url = "https://registry.example".to_string();
    config.identity.auth.oidc = serde_json::from_value(json!([
        {
            "name": "company",
            "issuer": "https://login.example",
            "audience": "pnpr",
            "login": { "users": [{ "subject": "alice", "username": "alice" }] },
        },
        {
            "name": "github",
            "issuer": "https://token.actions.githubusercontent.com",
            "audience": "pnpr",
            "workloads": [{
                "identity": { "subject": "repo:org/repo:ref:refs/heads/main", "username": "ci" },
                "registry": "local",
                "packages": ["foo"],
            }],
        }
    ]))
    .unwrap();
    config
}

fn config_with_multiple_login_providers(storage: &TempDir) -> Config {
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        storage.path().to_path_buf(),
    );
    config.http.public_url = "https://registry.example".to_string();
    config.identity.auth.oidc = serde_json::from_value(json!([
        {
            "name": "google",
            "issuer": "https://accounts.google.com",
            "audience": "pnpr",
            "login": { "users": [{ "subject": "alice", "username": "alice" }] },
        },
        {
            "name": "okta",
            "issuer": "https://company.okta.com",
            "audience": "pnpr",
            "login": { "users": [{ "subject": "bob", "username": "bob" }] },
        }
    ]))
    .unwrap();
    config
}

#[tokio::test]
async fn post_v1_login_returns_404_when_no_oidc_login_configured() {
    let storage = TempDir::new().unwrap();
    let app = router(config_without_oidc(&storage));

    let response = app
        .clone()
        .oneshot(
            Request::post("/-/v1/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let scoped_response = app
        .oneshot(
            Request::post("/~corp/-/v1/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(scoped_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_v1_login_returns_202_with_urls() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    let response = app
        .oneshot(
            Request::post("/-/v1/login")
                .header("npm-auth-type", "web")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");

    let body = body_json(response.into_body()).await;
    let login_url = body["loginUrl"].as_str().unwrap();
    let done_url = body["doneUrl"].as_str().unwrap();

    let browser_session =
        login_url.strip_prefix("https://registry.example/-/v1/login/start?session=").unwrap();
    let poll_token = done_url.strip_prefix("https://registry.example/-/v1/login/poll/").unwrap();
    assert_ne!(browser_session, poll_token);
}

#[tokio::test]
async fn post_v1_login_preserves_registry_prefix() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    let response = app
        .oneshot(
            Request::post("/~internal/-/v1/login")
                .header("npm-auth-type", "web")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let body = body_json(response.into_body()).await;
    let login_url = body["loginUrl"].as_str().unwrap();
    let done_url = body["doneUrl"].as_str().unwrap();

    let browser_session = login_url
        .strip_prefix("https://registry.example/~internal/-/v1/login/start?session=")
        .unwrap();
    let poll_token =
        done_url.strip_prefix("https://registry.example/~internal/-/v1/login/poll/").unwrap();
    assert_ne!(browser_session, poll_token);
}

#[tokio::test]
async fn polling_done_url_before_and_after_fulfillment() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    // 1. Initiate login
    let response = app
        .clone()
        .oneshot(
            Request::post("/-/v1/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(response.into_body()).await;
    let login_url = body["loginUrl"].as_str().unwrap();
    let done_url = body["doneUrl"].as_str().unwrap();
    let poll_token = done_url.strip_prefix("https://registry.example/-/v1/login/poll/").unwrap();
    let browser_session =
        login_url.strip_prefix("https://registry.example/-/v1/login/start?session=").unwrap();
    assert_ne!(browser_session, poll_token);

    // 2. Poll while pending using poll_token
    let poll_response = app
        .clone()
        .oneshot(
            Request::get(format!("/-/v1/login/poll/{poll_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(poll_response.status(), StatusCode::ACCEPTED);
    assert_eq!(poll_response.headers()[header::RETRY_AFTER], "1");
    assert_eq!(poll_response.headers()[header::CACHE_CONTROL], "private, no-store");

    // 3. Polling with browser_session returns 404
    let browser_poll_response = app
        .clone()
        .oneshot(
            Request::get(format!("/-/v1/login/poll/{browser_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(browser_poll_response.status(), StatusCode::NOT_FOUND);

    // 4. Start web login redirects to company provider with browser_session
    let start_response = app
        .clone()
        .oneshot(
            Request::get(login_url.strip_prefix("https://registry.example").unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::SEE_OTHER);
    let location = start_response.headers()[header::LOCATION].to_str().unwrap();
    assert_eq!(
        location,
        format!(
            "https://registry.example/-/oidc/company/login?return=cli&session={browser_session}"
        ),
    );

    // 5. Poll on unknown session returns 404
    let unknown_poll = app
        .clone()
        .oneshot(
            Request::get("/-/v1/login/poll/unknown_session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown_poll.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_v1_login_rate_limits_excessive_requests() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    for _ in 0..20 {
        let response = app
            .clone()
            .oneshot(
                Request::post("/-/v1/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    // 21st attempt is rate limited
    let throttled = app
        .oneshot(
            Request::post("/-/v1/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(throttled.headers().contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn start_with_multiple_providers_renders_picker() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_multiple_login_providers(&storage));

    let response = app
        .clone()
        .oneshot(
            Request::post("/-/v1/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(response.into_body()).await;
    let login_url = body["loginUrl"].as_str().unwrap();
    let start_path = login_url.strip_prefix("https://registry.example").unwrap();

    let start_response = app
        .oneshot(
            Request::get(start_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::OK);
    assert_eq!(start_response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");

    let html = String::from_utf8(body_bytes(start_response.into_body()).await).unwrap();
    assert!(html.contains("Sign in with google"));
    assert!(html.contains("Sign in with okta"));
    assert!(html.contains("/-/oidc/google/login?return=cli&amp;session="));
    assert!(html.contains("/-/oidc/okta/login?return=cli&amp;session="));
}

#[tokio::test]
async fn invalid_sessions_are_rejected() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    // Invalid start session
    let start_response = app
        .clone()
        .oneshot(
            Request::get("/-/v1/login/start?session=invalid_session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::BAD_REQUEST);

    // Invalid session parameter on provider login
    let login_response = app
        .clone()
        .oneshot(
            Request::get("/-/oidc/company/login?return=cli&session=invalid_session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::BAD_REQUEST);

    // Missing session parameter on provider login with return=cli
    let missing_session_response = app
        .oneshot(
            Request::get("/-/oidc/company/login?return=cli")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_session_response.status(), StatusCode::BAD_REQUEST);
}
