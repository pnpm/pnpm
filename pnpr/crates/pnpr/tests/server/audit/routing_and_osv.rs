use super::{
    Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, ServiceExt, StatusCode,
    TempDir, body_json, config_for, enable_osv, header, hosted_with_access, json, router,
    router_config,
};
use flate2::{Compression, write::GzEncoder};
use std::{fs, io::Write, path::PathBuf};

fn three_tier_router_config(npmjs_url: &str, corp_url: &str, storage: PathBuf) -> Config {
    let mut config = router_config(npmjs_url, corp_url, storage);
    config.routing.hosted.insert("local".to_string(), hosted_with_access("local", "$all"));
    let graph = vec![
        (
            "local".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        (
            "corp".to_string(),
            Registry::Upstream {
                patterns: vec![PackagePattern::parse("@corp/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "main".to_string(),
            Registry::Router {
                sources: vec!["local".to_string(), "corp".to_string(), "npmjs".to_string()],
            },
        ),
    ];
    let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    registries.validate().unwrap();
    config.routing.registries = registries;
    config
}

fn gzip_payload(raw: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}

#[tokio::test]
async fn router_bulk_advisories_partitions_by_source_and_never_leaks_hosted_packages() {
    let mut npmjs = mockito::Server::new_async().await;
    let mut corp = mockito::Server::new_async().await;

    let corp_mock = corp
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(r#"{"@corp/sdk":["2.0.0"]}"#)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "@corp/sdk": [{
                    "id": 201,
                    "url": "https://github.com/advisories/GHSA-corp-0001",
                    "title": "corp advisory",
                    "severity": "moderate",
                    "vulnerable_versions": "<2.1.0"
                }]
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let npmjs_mock = npmjs
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(r#"{"lodash":["4.17.20"]}"#)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "lodash": [{
                    "id": 101,
                    "url": "https://github.com/advisories/GHSA-npm-0001",
                    "title": "lodash advisory",
                    "severity": "high",
                    "vulnerable_versions": "<4.17.21"
                }],
                "@acme/private-pkg": [{
                    "id": 999,
                    "url": "https://github.com/advisories/GHSA-spoofed",
                    "title": "spoofed hosted advisory",
                    "severity": "critical",
                    "vulnerable_versions": "*"
                }]
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(three_tier_router_config(&npmjs.url(), &corp.url(), tmp.path().to_path_buf()));

    let query = json!({
        "@acme/private-pkg": ["1.0.0"],
        "@corp/sdk": ["2.0.0"],
        "lodash": ["4.17.20"]
    });
    let response = app
        .clone()
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
    assert!(body.get("@acme/private-pkg").is_none());
    assert_eq!(body["@corp/sdk"][0]["id"], 201);
    assert_eq!(body["lodash"][0]["id"], 101);

    let hosted_only = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"@acme/private-pkg":["1.0.0"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hosted_only.status(), StatusCode::OK);
    assert_eq!(body_json(hosted_only.into_body()).await, json!({}));

    corp_mock.assert_async().await;
    npmjs_mock.assert_async().await;
}

#[tokio::test]
async fn router_legacy_audits_strip_private_packages_across_requires_dependencies_and_packages() {
    let mut npmjs = mockito::Server::new_async().await;
    let mut corp = mockito::Server::new_async().await;

    let expected_corp_body = json!({
        "name": "pnpr-audit",
        "version": "0.0.0",
        "requires": { "@corp/sdk": "^2.0.0" },
        "dependencies": { "@corp/sdk": { "version": "2.0.0" } },
        "packages": {
            "": { "name": "pnpr-audit", "dependencies": { "@corp/sdk": "^2.0.0" } },
            "node_modules/@corp/sdk": { "version": "2.0.0" }
        }
    });
    let corp_audit = corp
        .mock("POST", mockito::Matcher::Regex("^/-/npm/v1/security/audits(/quick)?$".to_string()))
        .match_body(mockito::Matcher::Json(expected_corp_body))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "actions": [{ "module": "@corp/sdk", "action": "update" }],
                "advisories": {
                    "500": { "module_name": "@corp/sdk", "severity": "moderate" }
                },
                "muted": [],
                "metadata": {
                    "vulnerabilities": { "moderate": 1, "total": 1 },
                    "dependencies": 1,
                    "totalDependencies": 1
                }
            })
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let expected_npmjs_body = json!({
        "name": "pnpr-audit",
        "version": "0.0.0",
        "requires": { "lodash": "^4.17.20", "ms": "*" },
        "dependencies": {
            "lodash": { "version": "4.17.20" },
            "ms": { "version": "2.0.0" }
        },
        "packages": {
            "": { "name": "pnpr-audit", "dependencies": { "lodash": "^4.17.20" } },
            "node_modules/ms": { "version": "2.0.0" },
            "node_modules/lodash": { "version": "4.17.20" }
        }
    });
    let npmjs_audit = npmjs
        .mock("POST", mockito::Matcher::Regex("^/-/npm/v1/security/audits(/quick)?$".to_string()))
        .match_body(mockito::Matcher::Json(expected_npmjs_body))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "actions": [{ "module": "lodash", "action": "update" }],
                "advisories": {
                    "500": { "module_name": "lodash", "severity": "high" },
                    "999": { "module_name": "@acme/private-pkg", "severity": "critical" }
                },
                "muted": [],
                "metadata": {
                    "vulnerabilities": { "high": 1, "critical": 1, "total": 2 },
                    "dependencies": 2,
                    "totalDependencies": 2
                }
            })
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(three_tier_router_config(&npmjs.url(), &corp.url(), tmp.path().to_path_buf()));

    let payload = json!({
        "name": "@acme/workspace-root",
        "version": "1.0.0",
        "requires": {
            "@acme/private-pkg": "^1.0.0",
            "@corp/sdk": "^2.0.0",
            "lodash": "^4.17.20"
        },
        "dependencies": {
            "@acme/private-pkg": {
                "version": "1.0.0",
                "dependencies": {
                    "ms": { "version": "2.0.0" }
                }
            },
            "@corp/sdk": {
                "version": "2.0.0"
            },
            "lodash": {
                "version": "4.17.20",
                "requires": { "@acme/private-pkg": "^1.0.0" },
                "dependencies": {
                    "@acme/private-pkg": { "version": "1.0.0" }
                }
            }
        },
        "packages": {
            "": {
                "name": "@acme/workspace-root",
                "dependencies": {
                    "@acme/private-pkg": "^1.0.0",
                    "@corp/sdk": "^2.0.0",
                    "lodash": "^4.17.20"
                }
            },
            "node_modules/@acme/private-pkg": { "version": "1.0.0" },
            "node_modules/@acme/private-pkg/node_modules/ms": { "version": "2.0.0" },
            "node_modules/@corp/sdk": { "version": "2.0.0" },
            "node_modules/lodash": { "version": "4.17.20" }
        }
    });

    for path in ["/npm/-/npm/v1/security/audits", "/npm/-/npm/v1/security/audits/quick"] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let body = body_json(response.into_body()).await;
        assert_eq!(
            body["advisories"]
                .as_object()
                .unwrap()
                .len(),
            2,
            "{path}",
        );
        assert_eq!(body["metadata"]["vulnerabilities"]["critical"], 0, "{path}");
        assert_eq!(body["metadata"]["vulnerabilities"]["high"], 1, "{path}");
        assert_eq!(body["metadata"]["vulnerabilities"]["moderate"], 1, "{path}");
        assert_eq!(body["metadata"]["vulnerabilities"]["total"], 2, "{path}");
    }

    corp_audit.assert_async().await;
    npmjs_audit.assert_async().await;
}

#[tokio::test]
async fn audit_endpoints_support_gzip_and_reject_zip_bombs() {
    let mut upstream = mockito::Server::new_async().await;
    let bulk_mock = upstream
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(r#"{"lodash":["4.17.21"]}"#)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;
    let quick_mock = upstream
        .mock("POST", "/-/npm/v1/security/audits/quick")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"metadata":{"vulnerabilities":{"total":0}}}"#)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let gz_bulk = gzip_payload(br#"{"lodash":["4.17.21"]}"#);
    let res_bulk = app
        .clone()
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(Body::from(gz_bulk))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_bulk.status(), StatusCode::OK);

    let gz_quick =
        gzip_payload(br#"{"name":"app","dependencies":{"lodash":{"version":"4.17.21"}}}"#);
    let res_quick = app
        .clone()
        .oneshot(
            Request::post("/npm/-/npm/v1/security/audits/quick")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_ENCODING, "GZIP")
                .body(Body::from(gz_quick))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_quick.status(), StatusCode::OK);

    let unsupported = app
        .clone()
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_ENCODING, "br")
                .body(Body::from("anything"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let bomb = gzip_payload(&vec![b' '; 16 * 1024 * 1024 + 1024]);
    eprintln!("bomb compressed size: {}", bomb.len());
    assert!(bomb.len() < 128 * 1024);
    let too_large = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(Body::from(bomb))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);

    bulk_mock.assert_async().await;
    quick_mock.assert_async().await;
}

#[tokio::test]
async fn legacy_audits_evaluate_local_osv_for_hosted_and_router_registries() {
    let osv = TempDir::new().unwrap();
    fs::write(
        osv.path().join("GHSA-acme-0001.json"),
        json!({
            "id": "GHSA-acme-0001",
            "summary": "Hosted private advisory",
            "database_specific": { "severity": "CRITICAL", "cwe_ids": ["CWE-94"] },
            "affected": [{
                "package": { "ecosystem": "npm", "name": "@acme/private-pkg" },
                "ranges": [{
                    "type": "SEMVER",
                    "events": [{ "introduced": "0" }, { "fixed": "1.1.0" }]
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        osv.path().join("GHSA-pub-0002.json"),
        json!({
            "id": "GHSA-pub-0002",
            "summary": "Public lodash advisory",
            "database_specific": { "severity": "HIGH" },
            "affected": [{
                "package": { "ecosystem": "npm", "name": "lodash" },
                "ranges": [{
                    "type": "SEMVER",
                    "events": [{ "introduced": "0" }, { "fixed": "4.17.21" }]
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();

    let mut npmjs = mockito::Server::new_async().await;
    let npmjs_never_called = npmjs
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let corp = mockito::Server::new_async().await;

    let tmp = TempDir::new().unwrap();
    let mut config = three_tier_router_config(&npmjs.url(), &corp.url(), tmp.path().to_path_buf());
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let hosted_payload = gzip_payload(
        json!({
            "name": "hosted-check",
            "dependencies": {
                "@acme/private-pkg": { "version": "1.0.0" },
                "@acme/safe-pkg": { "version": "1.0.0" }
            }
        })
        .to_string()
        .as_bytes(),
    );
    let hosted_res = app
        .clone()
        .oneshot(
            Request::post("/npm/~local/-/npm/v1/security/audits")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(Body::from(hosted_payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hosted_res.status(), StatusCode::OK);
    let hosted_body = body_json(hosted_res.into_body()).await;
    assert_eq!(hosted_body["metadata"]["vulnerabilities"]["critical"], 1);
    assert_eq!(hosted_body["metadata"]["vulnerabilities"]["total"], 1);
    assert_eq!(hosted_body["metadata"]["totalDependencies"], 2);

    let router_payload = json!({
        "name": "router-check",
        "lockfileVersion": 3,
        "packages": {
            "": { "name": "router-check" },
            "node_modules/@acme/private-pkg": { "version": "1.0.0" },
            "node_modules/lodash": { "version": "4.17.20" }
        }
    });
    let router_res = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/audits/quick")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(router_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(router_res.status(), StatusCode::OK);
    let router_body = body_json(router_res.into_body()).await;
    assert_eq!(router_body["metadata"]["vulnerabilities"]["critical"], 1);
    assert_eq!(router_body["metadata"]["vulnerabilities"]["high"], 1);
    assert_eq!(router_body["metadata"]["vulnerabilities"]["total"], 2);
    assert_eq!(
        router_body["advisories"]
            .as_object()
            .unwrap()
            .len(),
        2,
    );

    npmjs_never_called.assert_async().await;
}

#[tokio::test]
async fn router_bulk_returns_not_found_when_any_queried_upstream_returns_404() {
    let mut npmjs = mockito::Server::new_async().await;
    let mut corp = mockito::Server::new_async().await;

    let corp_404 = corp
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(r#"{"@corp/sdk":["2.0.0"]}"#)
        .with_status(404)
        .expect(1)
        .create_async()
        .await;

    let npmjs_ok = npmjs
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(r#"{"lodash":["4.17.21"]}"#)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(three_tier_router_config(&npmjs.url(), &corp.url(), tmp.path().to_path_buf()));

    let query = json!({
        "@corp/sdk": ["2.0.0"],
        "lodash": ["4.17.21"],
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

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    corp_404.assert_async().await;
    npmjs_ok.assert_async().await;
}

#[tokio::test]
async fn router_legacy_audit_preserves_colliding_ms_versions_and_rewrites_hosted_aliases() {
    let mut npmjs = mockito::Server::new_async().await;
    let mut corp = mockito::Server::new_async().await;

    let corp_never = corp
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let expected_npmjs_body = json!({
        "name": "pnpr-audit",
        "lockfileVersion": 3,
        "requires": true,
        "packages": {
            "": { "name": "pnpr-audit" },
            "node_modules/ms": { "version": "2.1.3" },
            "node_modules/pnpr-redacted-scope-1/node_modules/ms": { "version": "2.0.0" },
            "node_modules/lodash": { "version": "4.17.20" },
        },
        "install": ["lodash@4.17.20"],
        "metadata": { "npm_version": "10.8.0" },
    });
    let npmjs_audit = npmjs
        .mock("POST", "/-/npm/v1/security/audits/quick")
        .match_body(mockito::Matcher::Json(expected_npmjs_body))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "actions": [],
                "advisories": {
                    "700": { "module_name": "ms", "severity": "moderate" },
                    "701": { "module_name": "lodash", "severity": "high" },
                },
                "muted": [],
                "metadata": {
                    "vulnerabilities": { "moderate": 1, "high": 1, "total": 2 },
                    "dependencies": 3,
                    "totalDependencies": 3,
                },
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(three_tier_router_config(&npmjs.url(), &corp.url(), tmp.path().to_path_buf()));

    let request_payload = json!({
        "name": "@acme/app",
        "lockfileVersion": 3,
        "requires": true,
        "packages": {
            "": { "name": "@acme/app" },
            "node_modules/@acme/private-pkg/node_modules/ms": { "version": "2.0.0" },
            "node_modules/ms": { "version": "2.1.3" },
            "node_modules/@acme/lodash-alias": { "version": "npm:lodash@4.17.20" },
            "node_modules/public-alias": { "version": "npm:@acme/private-pkg@1.0.0" },
            "node_modules/git-dep": {
                "version": "git+https://token@git.corp.internal/acme/secret.git",
            },
        },
        "install": [
            "public-alias@npm:@acme/private-pkg@1.0.0",
            "@acme/lodash-alias@npm:lodash@4.17.20",
        ],
        "metadata": {
            "npm_version": "10.8.0",
            "secret_workspace": "@acme/private-pkg",
        },
    });

    let response = app
        .oneshot(
            Request::post("/npm/-/npm/v1/security/audits/quick")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(request_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["metadata"]["vulnerabilities"]["total"], 2);
    corp_never.assert_async().await;
    npmjs_audit.assert_async().await;
}
