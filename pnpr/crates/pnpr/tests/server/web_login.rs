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
            "login": { "users": [{ "subject": "alice", "userId": "alice" }] },
        },
        {
            "name": "github",
            "issuer": "https://token.actions.githubusercontent.com",
            "audience": "pnpr",
            "workloads": [{
                "identity": { "subject": "repo:org/repo:ref:refs/heads/main", "userId": "ci" },
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
            "login": { "users": [{ "subject": "alice", "userId": "alice" }] },
        },
        {
            "name": "okta",
            "issuer": "https://company.okta.com",
            "audience": "pnpr",
            "login": { "users": [{ "subject": "bob", "userId": "bob" }] },
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

fn notice_code(response: &axum::response::Response) -> String {
    let notice = response.headers()["npm-notice"].to_str().unwrap();
    notice
        .strip_prefix("Enter the code ")
        .and_then(|rest| rest.strip_suffix(" in the browser to sign in"))
        .unwrap()
        .to_string()
}

/// Starts a web login at `path`, returning the response's user code,
/// `loginUrl`, and `doneUrl`.
async fn start_login_at(app: &axum::Router, path: &str) -> (String, String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("npm-auth-type", "web")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    let code = notice_code(&response);
    let body = body_json(response.into_body()).await;
    let url = |field: &str| {
        body[field]
            .as_str()
            .unwrap()
            .to_string()
    };
    (code, url("loginUrl"), url("doneUrl"))
}

fn submit_code(code: &str, provider: &str, site: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/-/v1/login/device")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(site) = site {
        request = request.header("sec-fetch-site", site);
    }
    request
        .body(Body::from(format!("code={code}&provider={provider}")))
        .unwrap()
}

#[tokio::test]
async fn post_v1_login_returns_a_code_and_a_login_url_without_secrets() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (code, login_url, done_url) = start_login_at(&app, "/-/v1/login").await;

    let (first, second) = code.split_once('-').unwrap();
    assert_eq!((first.len(), second.len()), (4, 4), "{code}");
    assert_eq!(login_url, "https://registry.example/-/v1/login/device");
    let poll_token = done_url.strip_prefix("https://registry.example/-/v1/login/poll/").unwrap();
    assert!(!poll_token.is_empty());
    assert!(!poll_token.contains(&code));
}

#[tokio::test]
async fn post_v1_login_preserves_registry_prefix() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (_, login_url, done_url) = start_login_at(&app, "/~internal/-/v1/login").await;
    assert_eq!(login_url, "https://registry.example/~internal/-/v1/login/device");
    assert!(done_url.starts_with("https://registry.example/~internal/-/v1/login/poll/"));
}

#[tokio::test]
async fn polling_stays_pending_until_the_login_completes() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (code, _, done_url) = start_login_at(&app, "/-/v1/login").await;
    let poll = |path: String| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    let pending = poll(
        done_url
            .strip_prefix("https://registry.example")
            .unwrap()
            .to_string(),
    )
    .await;
    assert_eq!(pending.status(), StatusCode::ACCEPTED);
    assert_eq!(pending.headers()[header::RETRY_AFTER], "1");
    assert_eq!(pending.headers()[header::CACHE_CONTROL], "private, no-store");

    for unknown in [code, "unknown_session".to_string()] {
        let response = poll(format!("/-/v1/login/poll/{unknown}")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
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
async fn the_device_page_asks_for_the_code_and_offers_each_provider() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_multiple_login_providers(&storage));
    let response = app
        .oneshot(
            Request::get("/-/v1/login/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");

    let html = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(html.contains(r#"<input name="code""#), "{html}");
    assert!(html.contains(r#"name="provider" value="google">Sign in with google<"#), "{html}");
    assert!(html.contains(r#"name="provider" value="okta">Sign in with okta<"#), "{html}");
}

#[tokio::test]
async fn the_device_page_is_absent_without_oidc_login() {
    let storage = TempDir::new().unwrap();
    let app = router(config_without_oidc(&storage));
    for request in [
        Request::get("/-/v1/login/device")
            .body(Body::empty())
            .unwrap(),
        submit_code("BCDF-GHJK", "company", Some("same-origin")),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request)
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND,
        );
    }
}

#[tokio::test]
async fn a_cli_login_cannot_be_completed_without_the_typed_code() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    let direct = app
        .clone()
        .oneshot(
            Request::get("/-/oidc/company/login?return=cli")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(direct.status(), StatusCode::BAD_REQUEST);

    let wrong = app
        .oneshot(submit_code("BCDF-GHJK", "company", Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert!(!wrong.headers().contains_key(header::SET_COOKIE));
    let html = String::from_utf8(body_bytes(wrong.into_body()).await).unwrap();
    assert!(html.contains("That code is unknown, expired, or already used."), "{html}");
}

#[tokio::test]
async fn the_code_is_accepted_only_from_a_same_origin_form_for_a_known_provider() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (code, _, _) = start_login_at(&app, "/-/v1/login").await;
    for request in [
        submit_code(&code, "company", None),
        submit_code(&code, "company", Some("cross-site")),
        submit_code(&code, "company", Some("same-site")),
        submit_code(&code, "github", Some("same-origin")),
        submit_code(&code, "unknown", Some("same-origin")),
    ] {
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
    }
}

#[tokio::test]
async fn code_guesses_are_rate_limited() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    for _ in 0..20 {
        let response = app
            .clone()
            .oneshot(submit_code("BCDF-GHJK", "company", Some("same-origin")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let throttled = app
        .oneshot(submit_code("BCDF-GHJK", "company", Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
}
