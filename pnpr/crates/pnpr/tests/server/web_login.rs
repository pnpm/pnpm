use super::{
    Body, Config, Ipv4Addr, Request, ServiceExt, SocketAddr, SocketAddrV4, StatusCode, TempDir,
    body_bytes, body_json, header, json, router,
};
use pnpr_fixtures::MockConfigPresets;

#[path = "web_login/device.rs"]
mod device;

pub(super) fn config_without_oidc(storage: &TempDir) -> Config {
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        storage.path().to_path_buf(),
    );
    config.http.public_url = "https://registry.example".to_string();
    config
}

pub(super) fn config_with_single_login_provider(storage: &TempDir) -> Config {
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

pub(super) fn config_with_multiple_login_providers(storage: &TempDir) -> Config {
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
pub(super) async fn start_login_at(app: &axum::Router, path: &str) -> (String, String, String) {
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

pub(super) fn submit_code(code: &str, provider: Option<&str>, site: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/-/v1/login/device")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(site) = site {
        request = request.header("sec-fetch-site", site);
    }
    let body = match provider {
        Some(provider) => format!("code={code}&provider={provider}"),
        None => format!("code={code}"),
    };
    request.body(Body::from(body)).unwrap()
}

pub(super) async fn html(response: axum::response::Response) -> String {
    String::from_utf8(body_bytes(response.into_body()).await).unwrap()
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
