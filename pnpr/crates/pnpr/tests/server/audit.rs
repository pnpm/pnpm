use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, body_json, config_for, enable_osv, header,
    json, router,
};
use std::fs;

fn sample_osv_database() -> TempDir {
    let dir = TempDir::new().unwrap();
    let advisory = json!({
        "id": "GHSA-audit-test",
        "summary": "Sample advisory for audit test",
        "database_specific": {
            "severity": "HIGH",
            "cwe_ids": ["CWE-79"]
        },
        "affected": [{
            "package": { "ecosystem": "npm", "name": "vulnerable" },
            "ranges": [{
                "type": "SEMVER",
                "events": [
                    { "introduced": "0" },
                    { "fixed": "2.0.0" }
                ]
            }]
        }],
    });
    fs::write(dir.path().join("GHSA-audit-test.json"), advisory.to_string()).unwrap();
    dir
}

#[tokio::test]
async fn bulk_advisories_with_local_osv_evaluates_database_and_returns_advisories() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("https://registry.npmjs.org/", tmp.path().to_path_buf());
    let osv = sample_osv_database();
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let query = json!({
        "vulnerable": ["1.0.0"],
        "safe": ["1.0.0"]
    });

    let response = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(query.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap(),
        "private, no-cache, no-store, must-revalidate",
    );

    let body = body_json(response.into_body()).await;
    assert!(body.get("safe").is_none());
    let vulns = body["vulnerable"].as_array().expect("array of advisories");
    assert_eq!(vulns.len(), 1);
    assert_eq!(vulns[0]["title"], "Sample advisory for audit test");
    assert_eq!(vulns[0]["severity"], "high");
    assert_eq!(vulns[0]["vulnerable_versions"], "<2.0.0");
    assert_eq!(vulns[0]["cwe"], json!(["CWE-79"]));
    assert!(vulns[0]["id"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn bulk_advisories_without_osv_proxies_to_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_header("content-type", "application/json")
        .match_body(r#"{"proxied":["1.0.0"]}"#)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "proxied": [{
                    "id": 999,
                    "url": "https://github.com/advisories/GHSA-upstream-test",
                    "title": "upstream advisory",
                    "severity": "critical",
                    "vulnerable_versions": "<2.0.0"
                }]
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    let query = json!({
        "proxied": ["1.0.0"]
    });

    let response = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(query.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["proxied"][0]["title"], "upstream advisory");
    assert_eq!(body["proxied"][0]["id"], 999);
    mock.assert_async().await;
}

#[tokio::test]
async fn bulk_advisories_scoped_registry_endpoint_proxies_to_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"scoped-test":[]}"#)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    let response = app
        .oneshot(
            Request::post("/npm/~npmjs/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"scoped-test":["1.0.0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["scoped-test"], json!([]));
    mock.assert_async().await;
}

#[tokio::test]
async fn legacy_audits_forward_to_upstream_when_configured() {
    let mut upstream = mockito::Server::new_async().await;
    let mock_full = upstream
        .mock("POST", "/-/npm/v1/security/audits")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"metadata":{"vulnerabilities":{"total":1}}}"#)
        .expect(1)
        .create_async()
        .await;

    let mock_quick = upstream
        .mock("POST", "/-/npm/v1/security/audits/quick")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"metadata":{"vulnerabilities":{"total":2}}}"#)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    let res_full = app
        .clone()
        .oneshot(
            Request::post("/npm/-/npm/v1/security/audits")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"test-project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_full.status(), StatusCode::OK);
    let body_full = body_json(res_full.into_body()).await;
    assert_eq!(body_full["metadata"]["vulnerabilities"]["total"], 1);

    let res_quick = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/audits/quick")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"test-project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_quick.status(), StatusCode::OK);
    let body_quick = body_json(res_quick.into_body()).await;
    assert_eq!(body_quick["metadata"]["vulnerabilities"]["total"], 2);

    mock_full.assert_async().await;
    mock_quick.assert_async().await;
}

#[tokio::test]
async fn legacy_audits_return_empty_report_when_upstream_returns_404() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("POST", "/-/npm/v1/security/audits/quick")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    let response = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/audits/quick")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"test-project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["metadata"]["vulnerabilities"]["total"], 0);
    mock.assert_async().await;
}

#[tokio::test]
async fn bulk_advisories_returns_empty_map_when_no_upstream_and_no_osv() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.routing.upstreams.clear();
    config.routing.hosted.insert("acme".to_string(), super::hosted_with_access("acme", "$all"));
    config.routing.registries = super::Registries::new(
        vec![("acme".to_string(), super::Registry::Hosted { patterns: vec![] })]
            .into_iter()
            .collect(),
        None,
    );
    let app = router(config);

    let response = app
        .oneshot(
            Request::post("/npm/~acme/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pkg":["1.0.0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body, json!({}));
}

#[tokio::test]
async fn legacy_audits_return_empty_report_when_no_upstream() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.routing.upstreams.clear();
    config.routing.hosted.insert("acme".to_string(), super::hosted_with_access("acme", "$all"));
    config.routing.registries = super::Registries::new(
        vec![("acme".to_string(), super::Registry::Hosted { patterns: vec![] })]
            .into_iter()
            .collect(),
        None,
    );
    let app = router(config);

    let response = app
        .oneshot(
            Request::post("/npm/~acme/-/npm/v1/security/audits")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"test-project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["metadata"]["vulnerabilities"]["total"], 0);
}
