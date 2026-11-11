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
    let code = notice_code(&response);
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

    // 4. The start page asks to confirm the code the CLI printed
    let start_path = login_url.strip_prefix("https://registry.example").unwrap();
    let start_response = app
        .clone()
        .oneshot(
            Request::get(start_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::OK);
    let html = String::from_utf8(body_bytes(start_response.into_body()).await).unwrap();
    assert!(html.contains(&format!("<strong>{code}</strong>")), "{html}");
    assert!(
        html.contains(&format!(
            r#"<form method="post" action="?session={browser_session}&amp;provider=company">"#,
        )),
        "{html}",
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
async fn start_with_multiple_providers_offers_each_provider() {
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
    assert!(html.contains(r#"&amp;provider=google">"#));
    assert!(html.contains(r#"&amp;provider=okta">"#));
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

    // A provider sign-in cannot complete a CLI session without the confirmation
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

fn notice_code(response: &axum::response::Response) -> String {
    let notice = response.headers()["npm-notice"].to_str().unwrap();
    notice
        .strip_prefix("Sign in only if the browser shows the code ")
        .unwrap()
        .to_string()
}

async fn start_login(app: &axum::Router) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::post("/-/v1/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let code = notice_code(&response);
    let body = body_json(response.into_body()).await;
    let session = body["loginUrl"]
        .as_str()
        .unwrap()
        .strip_prefix("https://registry.example/-/v1/login/start?session=")
        .unwrap()
        .to_string();
    (session, code)
}

fn confirm(
    session: &str,
    provider: &str,
    site: Option<&str>,
    origin: Option<&str>,
) -> Request<Body> {
    let mut request =
        Request::post(format!("/-/v1/login/start?session={session}&provider={provider}"));
    if let Some(site) = site {
        request = request.header("sec-fetch-site", site);
    }
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    request.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn post_v1_login_prints_the_confirmation_code() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (_, code) = start_login(&app).await;
    let (first, second) = code.split_once('-').unwrap();
    assert_eq!((first.len(), second.len()), (4, 4), "{code}");
}

#[tokio::test]
async fn a_cli_session_cannot_be_bound_to_a_provider_sign_in_without_confirmation() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (session, _) = start_login(&app).await;

    let direct = app
        .clone()
        .oneshot(
            Request::get(format!("/-/oidc/company/login?return=cli&session={session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(direct.status(), StatusCode::BAD_REQUEST);

    for request in [
        confirm(&session, "company", Some("cross-site"), None),
        confirm(&session, "company", Some("same-site"), Some("https://registry.example")),
        confirm(&session, "company", None, Some("https://attacker.example")),
        confirm(&session, "company", None, Some("null")),
        confirm(&session, "company", None, None),
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
async fn confirming_rejects_unknown_providers_and_sessions() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (session, _) = start_login(&app).await;
    for request in [
        confirm(&session, "github", Some("same-origin"), None),
        confirm(&session, "unknown", Some("same-origin"), None),
        confirm("unknown_session", "company", Some("same-origin"), None),
    ] {
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
