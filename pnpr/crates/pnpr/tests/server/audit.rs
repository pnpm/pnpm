#[path = "audit/routing_and_osv.rs"]
mod routing_and_osv;

use super::{
    Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, ServiceExt, StatusCode,
    TempDir, body_json, config_for, enable_osv, header, hosted_with_access, json, router,
    router_config,
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
    config.routing.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    config.routing.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
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
    config.routing.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    config.routing.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
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

#[tokio::test]
async fn audit_endpoints_reject_malformed_json_with_bad_request_before_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    for path in [
        "/npm/-/npm/v1/security/advisories/bulk",
        "/npm/-/npm/v1/security/audits",
        "/npm/-/npm/v1/security/audits/quick",
    ] {
        let request = Request::post(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{not-json"))
            .unwrap();
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let body = body_json(response.into_body()).await;
        assert_eq!(body["error"], "invalid json payload", "{path}");
    }

    mock.assert_async().await;
}

#[tokio::test]
async fn bulk_advisories_endpoint_preserves_osv_versions_cvss_severity_and_urls() {
    let osv = TempDir::new().unwrap();
    let ghsa_mixed = json!({
        "id": "GHSA-2222-3333-4444",
        "summary": "Critical advisory in @acme/vuln",
        "database_specific": { "severity": "CRITICAL" },
        "affected": [{
            "package": { "ecosystem": "npm", "name": "@acme/vuln" },
            "ranges": [{
                "type": "SEMVER",
                "events": [{ "introduced": "1.0.0" }, { "fixed": "1.0.5" }]
            }],
            "versions": ["2.0.1"]
        }]
    });
    let mal_cvss = json!({
        "id": "MAL-2026-0001",
        "summary": "CVSS 10.0 advisory",
        "severity": [{
            "type": "CVSS_V3",
            "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H"
        }],
        "affected": [{
            "package": { "ecosystem": "npm", "name": "@acme/vuln" },
            "ranges": [
                { "type": "GIT", "events": [{ "introduced": "0" }, { "fixed": "deadbeef" }] },
                { "type": "SEMVER", "events": [{ "introduced": "2.0.0" }, { "limit": "2.1.0" }] }
            ]
        }]
    });
    fs::write(osv.path().join("GHSA-2222-3333-4444.json"), ghsa_mixed.to_string()).unwrap();
    fs::write(osv.path().join("MAL-2026-0001.json"), mal_cvss.to_string()).unwrap();

    let tmp = TempDir::new().unwrap();
    let mut config = config_for("https://registry.npmjs.org/", tmp.path().to_path_buf());
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let query = json!({ "@acme/vuln": ["2.0.1"] });
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
    let advisories = body["@acme/vuln"].as_array().expect("advisories for @acme/vuln");
    assert_eq!(advisories.len(), 2);

    let target_version = node_semver::Version::parse("2.0.1").unwrap();
    for entry in advisories {
        let range_str = entry["vulnerable_versions"].as_str().unwrap();
        let range: node_semver::Range = range_str.parse().unwrap();
        dbg!(entry, &range);
        assert!(target_version.satisfies(&range));
        assert_eq!(entry["severity"], "critical");
    }

    let ghsa_entry = advisories
        .iter()
        .find(|item| item["title"] == "Critical advisory in @acme/vuln")
        .expect("GHSA entry");
    assert_eq!(ghsa_entry["vulnerable_versions"], ">=1.0.0 <1.0.5 || 2.0.1");
    assert_eq!(ghsa_entry["url"], "https://github.com/advisories/GHSA-2222-3333-4444");

    let mal_entry = advisories
        .iter()
        .find(|item| item["title"] == "CVSS 10.0 advisory")
        .expect("MAL entry");
    assert_eq!(mal_entry["vulnerable_versions"], ">=2.0.0 <2.1.0");
    assert_eq!(mal_entry["url"], "https://osv.dev/vulnerability/MAL-2026-0001");
}
