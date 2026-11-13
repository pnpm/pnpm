use super::{
    AccessList, Body, Registries, Registry, Request, ServiceExt, StatusCode, TempDir, config_for,
    drain_resolve_response, hosted_publish_request, hosted_with_access, json, package_tar_and_tgz,
    router_with_auth, sha512_integrity, spawn_counting_server,
};
use crate::tokens::issue_token;
use std::sync::atomic::Ordering;

async fn resolve(app: &axum::Router, (registry, spec): (&str, &str), token: &str) -> String {
    let request = Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "dependencies": { "private-widget": spec },
                "registry": registry,
                "preferFrozenLockfile": false,
            })
            .to_string(),
        ))
        .unwrap();
    let (status, body) = drain_resolve_response(
        app.clone()
            .oneshot(request)
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    String::from_utf8(body).unwrap()
}

#[tokio::test]
async fn resolve_private_hosted_package_in_process_as_caller() {
    let (public_url, requests) = spawn_counting_server(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.http.public_url = format!("{public_url}/registry/");
    config.routing.hosted.insert("internal".into(), hosted_with_access("internal", "reader"));
    config.routing.registries = Registries::new(
        [
            ("internal".into(), Registry::Hosted { patterns: vec![] }),
            ("private-router".into(), Registry::Router { sources: vec!["internal".into()] }),
        ]
        .into_iter()
        .collect(),
        Some("internal".into()),
    );
    let auth =
        crate::tokens::seeded_auth(&config, &[("reader", "password"), ("outsider", "password")]);
    let reader = issue_token(&auth, "reader").await;
    let outsider = issue_token(&auth, "outsider").await;
    let app = router_with_auth(config, auth);
    let (_, tgz) = package_tar_and_tgz(r#"{"name":"private-widget","version":"1.0.0"}"#);
    let published = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~internal/private-widget",
            "private-widget",
            "1.0.0",
            &tgz,
            &reader,
        ))
        .await
        .unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);
    for path in ["npm/", "npm/~internal/", "npm/~%69nternal/", "npm/~private-router/"] {
        let registry = format!("{public_url}/registry/{path}");
        let body = resolve(&app, (&registry, "1.0.0"), &reader).await;
        assert!(!body.contains(r#""type":"error""#), "{body}");
        assert!(body.contains(r#""type":"done""#), "{body}");
        let denied = resolve(&app, (&registry, "1.0.0"), &outsider).await;
        assert!(denied.contains(r#""type":"error""#), "{denied}");
        let tarball = format!("{registry}private-widget/-/private-widget-1.0.0.tgz");
        let direct = resolve(&app, (&registry, &tarball), &reader).await;
        assert!(direct.contains(r#""type":"done""#), "{direct}");
        let denied = resolve(&app, (&registry, &tarball), &outsider).await;
        assert!(denied.contains(r#""type":"error""#), "{denied}");
    }
    assert_eq!(requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolve_private_upstream_sends_credential_only_to_upstream() {
    resolve_private_upstream("authorization").await;
}

#[tokio::test]
async fn resolve_private_upstream_with_custom_header_keeps_cache_private() {
    resolve_private_upstream("x-api-key").await;
}

async fn resolve_private_upstream(header: &str) {
    let (public_url, requests) = spawn_counting_server(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    // The upstream is on `127.0.0.1` too, which as the own host it could not reach.
    let public_url = public_url.replace("://127.0.0.1:", "://localhost:");
    let mut upstream = mockito::Server::new_async().await;
    let (_, tgz) = package_tar_and_tgz(r#"{"name":"private-widget","version":"1.0.0"}"#);
    let packument = upstream.mock("GET", "/private-widget")
        .match_header(header, "Bearer upstream-test-token")
        .with_header("content-type", "application/json")
        .with_body(json!({
            "name": "private-widget",
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": {
                "name": "private-widget", "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/private-widget/-/private-widget-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(&tgz),
                },
            } },
        }).to_string())
        .expect_at_least(1).create_async().await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.public_url = public_url.clone();
    let source = config.routing.upstreams.get_mut("npmjs").unwrap();
    source.access = Some(AccessList::from_tokens(["reader"]));
    source.headers.insert(
        reqwest::header::HeaderName::from_bytes(header.as_bytes()).unwrap(),
        "Bearer upstream-test-token".parse().unwrap(),
    );
    let auth =
        crate::tokens::seeded_auth(&config, &[("reader", "password"), ("outsider", "password")]);
    let reader = issue_token(&auth, "reader").await;
    let outsider = issue_token(&auth, "outsider").await;
    let app = router_with_auth(config, auth);
    let registry = format!("{public_url}/npm/~npmjs/");
    let body = resolve(&app, (&registry, "1.0.0"), &reader).await;
    assert!(!body.contains(r#""type":"error""#), "{body}");
    assert!(body.contains(r#""type":"done""#), "{body}");
    let denied = resolve(&app, (&registry, "1.0.0"), &outsider).await;
    assert!(denied.contains(r#""type":"error""#), "{denied}");
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    packument.assert_async().await;
}
