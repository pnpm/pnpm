use super::{
    HANDOFF_COOKIE, LoginReturn, LoginSession, OidcState, check_workload_request, redeem,
    signed_in, validate_workloads,
};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, header},
};
use pnpr_config::Config;
use std::net::SocketAddr;
use tower::ServiceExt as _;

const CONFIG: &str = "
registries:
  private:
    type: hosted
    packages:
      '@org/*':
        publish: [ci]
auth:
  oidc:
    - name: github
      issuer: https://token.actions.githubusercontent.com
      audience: https://registry.example
      workloads:
        - identity:
            subject: repo:org/repo:ref:refs/heads/main
            username: ci
            claims:
              repository_id: '123'
          registry: private
          packages: ['@org/pkg']
";

fn config(yaml: &str) -> Config {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    Config::from_yaml(&path, "127.0.0.1:0".parse::<SocketAddr>().unwrap(), None).unwrap()
}

#[test]
fn workload_credentials_only_permit_named_package_publication() {
    let config = config(CONFIG);
    validate_workloads(&config).unwrap();
    let workload = &config.identity.auth.oidc[0].workloads[0];
    for path in ["/~private/@org/pkg", "/~private/@org%2fpkg", "/~private/%40org%2Fpkg"] {
        check_workload_request(&config, workload, &Method::PUT, path).unwrap();
    }
    for path in [
        "/@org/pkg",
        "/~other/@org/pkg",
        "/~private/@org/other",
        "/~private/@org/pkg/-rev/1",
        "/~private/-/package/@org%2fpkg/dist-tags/latest",
        "/-/user/org.couchdb.user:ci",
        "/-/npm/v1/tokens",
        "/-/pnpr/v0/publish",
        "/-/pnpm/v1/publish",
        "/v2/token",
        "/~private/@org%252fpkg",
        "/~private/@org/pkg/",
        "/~private/@org/pkg/../other",
    ] {
        assert!(
            check_workload_request(&config, workload, &Method::PUT, path).is_err(),
            "accepted {path}",
        );
    }
    for method in [Method::GET, Method::POST, Method::DELETE, Method::PATCH] {
        assert!(
            check_workload_request(&config, workload, &method, "/~private/@org/pkg").is_err(),
            "accepted {method}",
        );
    }
}

#[test]
fn workload_targets_validate_at_startup() {
    let mut config = config(CONFIG);
    config.identity.auth.oidc[0].workloads[0].registry = "unknown".to_string();
    assert!(validate_workloads(&config).is_err());
    config.identity.auth.oidc[0].workloads[0].registry = "private".to_string();
    config.identity.auth.oidc[0].workloads[0].packages = vec!["@org/*".to_string()];
    assert!(validate_workloads(&config).is_err());
    config.identity.auth.oidc[0].workloads[0].packages.clear();
    assert!(validate_workloads(&config).is_err());
}

#[test]
fn callback_secrets_never_appear_in_request_logs() {
    let uri: Uri = "/-/oidc/example/callback?code=secret&state=secret-state".parse().unwrap();
    assert_eq!(super::super::loggable_uri(&uri), "/-/oidc/example/callback");
}

#[tokio::test]
async fn invalid_oidc_credentials_fail_closed_on_public_endpoints() {
    let mut config = config(CONFIG);
    let storage = tempfile::TempDir::new().unwrap();
    config.storage.hosted_dir = storage.path().to_path_buf();
    let app = crate::try_router(config).unwrap();
    for token in ["pnpr_oidc_unknown", "pnpr_workload_e30.e30.invalid"] {
        let response = app
            .clone()
            .oneshot(
                Request::get("/-/ping")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .oneshot(
            Request::get("/-/oidc/github/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert!(
        response.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store"),
    );
}

#[tokio::test]
async fn a_ui_sign_in_hands_the_token_only_to_the_browser_that_signed_in() {
    let oidc = OidcState::new(&[], "https://registry.example").unwrap();
    let session = LoginSession {
        token: "pnpr_oidc_session".to_string(),
        expires: chrono::Utc::now().timestamp() + 60,
        returns_to: LoginReturn::Ui,
    };
    let landed = signed_in(&oidc, session);
    assert!(landed.status().is_redirection());
    assert_eq!(landed.headers()[header::LOCATION], "../../ui/sign-in/oidc");
    let cookie = landed.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("; HttpOnly;"), "{cookie}");
    let pair = cookie.split(';').next().unwrap();
    assert!(pair.starts_with(&format!("{HANDOFF_COOKIE}=")), "{cookie}");
    let with_cookie = |pair: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(pair).unwrap());
        headers
    };

    for refused in [HeaderMap::new(), with_cookie(&format!("{HANDOFF_COOKIE}=other-code"))] {
        assert_eq!(redeem(&oidc, &refused).status(), StatusCode::UNAUTHORIZED);
    }
    let redeemed = redeem(&oidc, &with_cookie(pair));
    assert_eq!(redeemed.status(), StatusCode::OK);
    assert!(
        redeemed.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .ends_with("Max-Age=0"),
    );
    let body = axum::body::to_bytes(redeemed.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["token"], "pnpr_oidc_session");
    assert_eq!(redeem(&oidc, &with_cookie(pair)).status(), StatusCode::UNAUTHORIZED);
}
