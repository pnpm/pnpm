use super::{
    HANDOFF_COOKIE_PREFIX, LoginReturn, LoginSession, OidcState, WorkloadScope,
    check_workload_request, redeem, signed_in, validate_workloads,
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
    for path in ["/npm/~private/@org/pkg", "/npm/~private/@org%2fpkg", "/npm/~private/%40org%2Fpkg"]
    {
        check_workload_request(workload, &Method::PUT, path).unwrap();
    }
    for path in [
        "/~private/@org/pkg",
        "/npm/@org/pkg",
        "/npm/~other/@org/pkg",
        "/npm/~private/@org/other",
        "/npm/~private/@org/pkg/-rev/1",
        "/npm/~private/-/package/@org%2fpkg/dist-tags/latest",
        "/-/user/org.couchdb.user:ci",
        "/-/npm/v1/tokens",
        "/npm/~other/-/pnpr/v0/publish",
        "/npm/~other/-/pnpr/v0/artifacts/blob",
        "/-/pnpr/v0/publish/extra",
        "/v2/token",
        "/npm/~private/@org%252fpkg",
        "/npm/~private/@org/pkg/",
        "/npm/~private/@org/pkg/../other",
    ] {
        assert!(check_workload_request(workload, &Method::PUT, path).is_err(), "accepted {path}");
    }
    for method in [Method::GET, Method::POST, Method::DELETE, Method::PATCH] {
        assert!(
            check_workload_request(workload, &method, "/npm/~private/@org/pkg").is_err(),
            "accepted {method}",
        );
    }
}

/// A workload stages archives and commits a publish that references them,
/// through the server root, the npm base, or its registry's address. The
/// commit checks each entry against the workload's packages.
#[test]
fn workload_credentials_permit_staging_and_publish_commits() {
    let config = config(CONFIG);
    let workload = &config.identity.auth.oidc[0].workloads[0];
    for base in ["", "/npm", "/npm/~private"] {
        for method in [Method::PUT, Method::HEAD] {
            let path = format!("{base}/-/pnpr/v0/artifacts/blob");
            check_workload_request(workload, &method, &path).unwrap();
        }
        let path = format!("{base}/-/pnpr/v0/publish");
        check_workload_request(workload, &Method::PUT, &path).unwrap();
        for method in [Method::GET, Method::POST, Method::DELETE] {
            assert!(check_workload_request(workload, &method, &path).is_err(), "{method} {path}");
        }
    }
    let scope = WorkloadScope::new(&config, workload);
    scope.check_ecosystem(pnpr_registry::Ecosystem::Npm).unwrap();
    assert!(scope.check_ecosystem(pnpr_registry::Ecosystem::Cargo).is_err());
    let private =
        config.routing.registries.addressed("private", pnpr_registry::Ecosystem::Npm).unwrap();
    scope.check_npm_publish(private, "@org/pkg").unwrap();
    assert!(scope.check_npm_publish(private, "@org/other").is_err());
    assert!(scope.check_npm_publish("npm/other", "@org/pkg").is_err());
}

/// A batch sent with workload credentials publishes nothing outside the
/// workload's packages, whichever address it is sent to.
#[tokio::test]
async fn a_workload_batch_is_scoped_to_its_packages() {
    let mut config = config(CONFIG);
    let storage = tempfile::TempDir::new().unwrap();
    config.storage.hosted_dir = storage.path().to_path_buf();
    let scope = WorkloadScope::new(&config, &config.identity.auth.oidc[0].workloads[0]);
    let record = pnpr_auth::TokenRecord::new("ci", 1, 1);
    let app = super::super::tests::app_with_config_and_token(config, "tok", record)
        .layer(axum::Extension(scope));
    let send = |path: &str, body: serde_json::Value| {
        let app = app.clone();
        let request = Request::put(path)
            .header(header::AUTHORIZATION, "Bearer tok")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        async move { app.oneshot(request).await.unwrap() }
    };
    let entry = |name: &str| {
        serde_json::json!({
            "name": name,
            "versions": { "1.0.0": { "name": name, "version": "1.0.0" } },
        })
    };
    let invalid_version = send(
        "/npm/~private/-/pnpr/v0/publish",
        serde_json::json!({
            "packages": [{
                "name": "@org/other",
                "versions": { "banana": { "name": "@org/other", "version": "banana" } },
            }],
        }),
    )
    .await;
    assert_eq!(invalid_version.status(), StatusCode::BAD_REQUEST);
    let other = send(
        "/npm/~private/-/pnpr/v0/publish",
        serde_json::json!({ "packages": [entry("@org/other")] }),
    )
    .await;
    assert_eq!(other.status(), StatusCode::FORBIDDEN);
    for path in ["/npm/~private/-/pnpr/v0/publish", "/npm/-/pnpr/v0/publish", "/-/pnpr/v0/publish"]
    {
        let crate_entry = serde_json::json!({
            "packages": [{ "ecosystem": "cargo", "metadata": {}, "archive": "" }],
        });
        assert_eq!(send(path, crate_entry).await.status(), StatusCode::FORBIDDEN, "{path}");
    }
    // The workload's own package passes the scope, and fails only for want
    // of an archive.
    let own = send(
        "/npm/~private/-/pnpr/v0/publish",
        serde_json::json!({ "packages": [entry("@org/pkg")] }),
    )
    .await;
    assert_eq!(own.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(own.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("without a tarball"));
}

/// Staging is per principal, so a workload stages through its registry's
/// address as through the npm base.
#[tokio::test]
async fn archives_are_staged_under_a_registry_address() {
    use base64::Engine as _;
    use sha2::Digest as _;

    let storage = tempfile::TempDir::new().unwrap();
    let mut config = config(CONFIG);
    config.storage.hosted_dir = storage.path().to_path_buf();
    let record = pnpr_auth::TokenRecord::new("ci", 1, 1);
    let app = super::super::tests::app_with_config_and_token(config, "tok", record);
    let bytes = b"archive bytes".to_vec();
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&bytes)),
    );
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("integrity", &integrity)
        .finish();
    let path = format!("/npm/~private/-/pnpr/v0/artifacts/blob?{query}");
    let put = Request::put(&path)
        .header(header::AUTHORIZATION, "Bearer tok")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes.clone()))
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(put)
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED,
    );
    let head = Request::head(format!("/npm/-/pnpr/v0/artifacts/blob?{query}"))
        .header(header::AUTHORIZATION, "Bearer tok")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(head).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], bytes.len().to_string());
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
async fn a_ui_sign_in_hands_the_token_only_to_the_browser_and_flow_that_signed_in() {
    let oidc = OidcState::new(&[], "https://registry.example").unwrap();
    let sign_in = |flow: &str, token: &str| {
        let session = LoginSession {
            token: token.to_string(),
            expires: chrono::Utc::now().timestamp() + 60,
            returns_to: LoginReturn::Ui,
        };
        let landed = signed_in(&oidc, flow, session);
        assert!(landed.status().is_redirection());
        assert_eq!(
            landed.headers()[header::LOCATION],
            format!("../../ui/sign-in/oidc?flow={flow}"),
        );
        let cookie = landed.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_string();
        assert!(cookie.starts_with(&format!("{HANDOFF_COOKIE_PREFIX}{flow}=")), "{cookie}");
        assert!(cookie.contains("; HttpOnly;"), "{cookie}");
        cookie
            .split(';')
            .next()
            .unwrap()
            .to_string()
    };
    let first = sign_in("flow-a", "pnpr_oidc_first");
    let second = sign_in("flow-b", "pnpr_oidc_second");
    let with_cookies = |pairs: &[&str]| {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(&pairs.join("; ")).unwrap());
        headers
    };
    let token = |response: axum::response::Response| async move {
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["token"].clone()
    };

    assert_eq!(redeem(&oidc, "flow-a", &HeaderMap::new()).status(), StatusCode::UNAUTHORIZED);
    let planted = with_cookies(&[&format!("{HANDOFF_COOKIE_PREFIX}flow-a=other-code")]);
    assert_eq!(redeem(&oidc, "flow-a", &planted).status(), StatusCode::UNAUTHORIZED);
    let both = with_cookies(&[&first, &second]);
    let redeemed = redeem(&oidc, "flow-a", &both);
    assert!(
        redeemed.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .starts_with(&format!("{HANDOFF_COOKIE_PREFIX}flow-a=;")),
    );
    assert_eq!(token(redeemed).await, "pnpr_oidc_first");
    assert_eq!(redeem(&oidc, "flow-a", &both).status(), StatusCode::UNAUTHORIZED);
    assert_eq!(token(redeem(&oidc, "flow-b", &both)).await, "pnpr_oidc_second");
}
