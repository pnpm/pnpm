use std::{
    collections::{HashMap, HashSet},
    io::Write,
};

use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, StatusCode, header},
};
use flate2::{Compression, write::GzEncoder};
use pnpr_osv::OsvIndex;
use serde_json::{Map, Value, json};

use super::{
    PackageDestination,
    encoding::{decode_request_body, decompress_gzip_bounded},
    legacy_merge::{merge_upstream_legacy_responses, sanitize_upstream_legacy_response},
    legacy_nodes::collect_installed_nodes,
    legacy_report::{build_osv_legacy_report, derive_ghsa_id, infer_patched_versions},
    legacy_sanitize::build_upstream_legacy_payload,
    legacy_specs::{
        is_safe_semver_range, is_safe_semver_version, is_safe_sri_integrity, parse_name_and_spec,
        parse_npm_alias,
    },
};

fn gzip_bytes(payload: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(payload).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn decode_request_body_handles_identity_and_case_insensitive_gzip() {
    let raw = Bytes::from_static(br#"{"lodash":["4.17.21"]}"#);
    let empty_headers = HeaderMap::new();
    let identity = decode_request_body(&empty_headers, &raw).expect("identity succeeds");
    assert_eq!(identity.as_ref(), raw.as_ref());

    for coding in ["gzip", "GZIP", " x-gzip "] {
        let compressed = Bytes::from(gzip_bytes(raw.as_ref()));
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_str(coding).unwrap());
        let decoded = decode_request_body(&headers, &compressed).expect("gzip decodes");
        assert_eq!(decoded.as_ref(), raw.as_ref());
    }
}

#[test]
fn decode_request_body_rejects_unsupported_corrupt_and_oversized_streams() {
    let raw = Bytes::from_static(b"not-gzip");
    for coding in ["br", "deflate", "gzip, gzip"] {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(coding));
        let err = decode_request_body(&headers, &raw).expect_err("unsupported coding rejected");
        assert_eq!(err.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    let mut multi_headers = HeaderMap::new();
    multi_headers.append(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    multi_headers.append(header::CONTENT_ENCODING, HeaderValue::from_static("identity"));
    let multi_err = decode_request_body(&multi_headers, &raw).expect_err("multiple headers");
    assert_eq!(multi_err.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let mut gzip_headers = HeaderMap::new();
    gzip_headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    let corrupt_err = decode_request_body(&gzip_headers, &raw).expect_err("corrupt gzip");
    assert_eq!(corrupt_err.status(), StatusCode::BAD_REQUEST);

    let empty_err =
        decode_request_body(&gzip_headers, &Bytes::new()).expect_err("empty gzip rejected");
    assert_eq!(empty_err.status(), StatusCode::BAD_REQUEST);

    let mut valid_plus_garbage = gzip_bytes(b"{}");
    valid_plus_garbage.extend_from_slice(b"trailing-garbage");
    let trailing_err =
        decompress_gzip_bounded(&valid_plus_garbage, 1024).expect_err("trailing garbage rejected");
    assert_eq!(trailing_err.status(), StatusCode::BAD_REQUEST);

    let bomb = gzip_bytes(&vec![b'a'; 4096]);
    let bomb_err = decompress_gzip_bounded(&bomb, 1024).expect_err("oversized stream rejected");
    assert_eq!(bomb_err.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn build_upstream_legacy_payload_strips_private_packages_and_promotes_public_children() {
    let payload: Map<String, Value> = serde_json::from_value(json!({
        "name": "@acme/secret-app",
        "version": "2.3.4",
        "requires": {
            "@acme/private-pkg": "^1.0.0",
            "lodash": "^4.17.20",
            "aliased-private": "npm:@acme/private-pkg@1.0.0"
        },
        "dependencies": {
            "@acme/private-pkg": {
                "version": "1.0.0",
                "resolved": "https://pnpr.internal/npm/~local/@acme/private-pkg/-/private-pkg-1.0.0.tgz",
                "dependencies": {
                    "@acme/deep-secret": { "version": "0.1.0" },
                    "ms": { "version": "2.0.0" }
                }
            },
            "lodash": {
                "version": "4.17.20",
                "requires": {
                    "@acme/private-pkg": "^1.0.0",
                    "minimist": "^1.2.5"
                },
                "dependencies": {
                    "@acme/private-pkg": { "version": "1.0.0" },
                    "minimist": { "version": "1.2.5" }
                }
            }
        },
        "packages": {
            "": {
                "name": "@acme/secret-app",
                "version": "2.3.4",
                "dependencies": {
                    "@acme/private-pkg": "^1.0.0",
                    "lodash": "^4.17.20"
                }
            },
            "packages/secret-workspace": {
                "name": "@acme/secret-workspace",
                "version": "1.0.0"
            },
            "packages/secret-workspace/node_modules/@acme/private-pkg/node_modules/ms": {
                "version": "2.0.0"
            },
            "node_modules/@acme/private-pkg": {
                "version": "1.0.0"
            },
            "node_modules/lodash": {
                "version": "4.17.20"
            }
        },
        "install": ["@acme/private-pkg@1.0.0", "lodash@4.17.20"],
        "remove": ["@acme/private-pkg"],
        "secretCustomField": "@acme/private-pkg"
    }))
    .unwrap();

    let mut destinations = HashMap::new();
    destinations.insert("@acme/secret-app".to_string(), PackageDestination::Hosted);
    destinations.insert("@acme/secret-workspace".to_string(), PackageDestination::Hosted);
    destinations.insert("@acme/private-pkg".to_string(), PackageDestination::Hosted);
    destinations.insert("@acme/deep-secret".to_string(), PackageDestination::Hosted);
    destinations.insert(
        "aliased-private".to_string(),
        PackageDestination::Upstream("npmjs".to_string()),
    );
    destinations.insert("lodash".to_string(), PackageDestination::Upstream("npmjs".to_string()));
    destinations.insert("ms".to_string(), PackageDestination::Upstream("npmjs".to_string()));
    destinations.insert("minimist".to_string(), PackageDestination::Upstream("npmjs".to_string()));

    let sanitized = build_upstream_legacy_payload(&payload, "npmjs", &destinations);
    let serialized = serde_json::to_string(&sanitized).unwrap();
    assert!(!serialized.contains("@acme"));
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("pnpr.internal"));
    assert_eq!(sanitized["name"], "pnpr-audit");
    assert_eq!(sanitized["dependencies"]["ms"]["version"], "2.0.0");
    assert_eq!(sanitized["dependencies"]["lodash"]["dependencies"]["minimist"]["version"], "1.2.5");
    assert_eq!(sanitized["packages"]["node_modules/ms"]["version"], "2.0.0");
    assert_eq!(sanitized["install"], json!(["lodash@4.17.20"]));
    assert_eq!(sanitized["remove"], json!([]));
}

#[test]
fn legacy_sanitizer_preserves_colliding_versions_rewrites_aliases_and_blocks_url_and_metadata_leaks()
 {
    let payload: Map<String, Value> = serde_json::from_value(json!({
        "name": "@acme/app",
        "requires": {
            "ms": "^2.1.3",
            "@acme/lodash-alias": "npm:lodash@^4.17.20",
            "public-alias": "npm:@acme/private-pkg@9.9.9",
            "git-fork": "acme-org/private-repo",
        },
        "dependencies": {
            "ms": { "version": "2.1.3" },
            "@acme/private-pkg": {
                "version": "1.0.0",
                "dependencies": {
                    "ms": { "version": "2.0.0" },
                },
            },
            "@acme/lodash-alias": {
                "version": "npm:lodash@4.17.20",
            },
            "public-alias": {
                "version": "npm:@acme/private-pkg@9.9.9",
            },
            "minimist": {
                "version": "git+https://user:secret-token@git.corp.internal/org/minimist.git",
            },
            "tarball-dep": {
                "version": "https://corp.internal/tarball-dep-1.0.0.tgz",
            },
            "file-dep": {
                "version": "file:../private-local",
            },
        },
        "packages": {
            "": { "name": "@acme/app" },
            "node_modules/@acme/private-pkg/node_modules/ms": { "version": "2.0.0" },
            "node_modules/ms": { "version": "2.1.3" },
            "node_modules/@acme/lodash-alias": { "version": "npm:lodash@4.17.20" },
            "node_modules/public-alias": { "version": "npm:@acme/private-pkg@9.9.9" },
            "node_modules/minimist": {
                "version": "git+https://user:secret-token@git.corp.internal/org/minimist.git",
            },
            "node_modules/file-dep": { "version": "file:../private-local" },
        },
        "install": [
            "public-alias@npm:@acme/private-pkg@9.9.9",
            "@acme/lodash-alias@npm:lodash@4.17.20",
            "minimist@git+https://user:secret-token@git.corp.internal/org/minimist.git",
            "ms@2.1.3",
        ],
        "remove": [
            "public-alias@npm:@acme/private-pkg@9.9.9",
        ],
        "metadata": {
            "npm_version": "10.8.0",
            "node_version": "v22.0.0",
            "private_project": "@acme/private-pkg",
            "repo_url": "https://git.corp.internal/acme/secret",
            "nested": { "secret": "@acme/private-pkg" },
        },
    }))
    .unwrap();

    let destinations = HashMap::from([
        ("@acme/app".to_string(), PackageDestination::Hosted),
        ("@acme/private-pkg".to_string(), PackageDestination::Hosted),
        ("@acme/lodash-alias".to_string(), PackageDestination::Hosted),
        ("ms".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("lodash".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("public-alias".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("git-fork".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("minimist".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("tarball-dep".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("file-dep".to_string(), PackageDestination::Upstream("npmjs".to_string())),
    ]);

    let sanitized = build_upstream_legacy_payload(&payload, "npmjs", &destinations);
    let serialized = serde_json::to_string(&sanitized).unwrap();
    assert!(!serialized.contains("@acme"));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("corp.internal"));
    assert!(!serialized.contains("private-local"));
    assert!(!serialized.contains("private-repo"));
    assert!(!serialized.contains("public-alias"));

    assert_eq!(sanitized["dependencies"]["ms"]["version"], "2.1.3");
    assert_eq!(
        sanitized["dependencies"]["pnpr-redacted-scope-1"]["dependencies"]["ms"]["version"],
        "2.0.0",
    );
    assert_eq!(sanitized["dependencies"]["lodash"]["version"], "4.17.20");
    assert!(sanitized["dependencies"].get("minimist").is_none());
    assert!(sanitized["dependencies"].get("tarball-dep").is_none());
    assert!(sanitized["dependencies"].get("file-dep").is_none());

    assert_eq!(sanitized["packages"]["node_modules/ms"]["version"], "2.1.3");
    assert_eq!(
        sanitized["packages"]["node_modules/pnpr-redacted-scope-1/node_modules/ms"]["version"],
        "2.0.0",
    );
    assert_eq!(sanitized["packages"]["node_modules/lodash"]["version"], "4.17.20");
    assert!(sanitized["packages"].get("node_modules/minimist").is_none());
    assert!(sanitized["packages"].get("node_modules/file-dep").is_none());

    assert_eq!(sanitized["requires"]["lodash"], "^4.17.20");
    assert_eq!(sanitized["install"], json!(["lodash@4.17.20", "ms@2.1.3"]));
    assert_eq!(sanitized["remove"], json!([]));
    assert_eq!(
        sanitized["metadata"],
        json!({
            "npm_version": "10.8.0",
            "node_version": "v22.0.0",
        }),
    );
}

#[test]
fn legacy_sanitizer_rejects_trailing_spec_tokens_fake_integrity_and_scales_linearly() {
    let valid_sri = "sha512-2Z9S0000000000000000000000000000==";
    assert!(is_safe_sri_integrity(valid_sri));
    assert!(!is_safe_sri_integrity("https://user:secret-token@corp.internal/hash"));
    assert!(!is_safe_sri_integrity("sha512-2Z9S00000000000000000000==?@acme/private-pkg"));
    assert!(!is_safe_semver_range("1.2.3 secret-package"));
    assert!(!is_safe_semver_range("^1.0.0 @acme/private-pkg"));
    assert!(!is_safe_semver_version("1.2.3 secret-package"));
    assert!(is_safe_semver_range(">=1.0.0 <2.0.0 || 3.x"));

    let mut packages = Map::new();
    packages.insert(String::new(), json!({ "name": "app" }));
    packages.insert(
        "node_modules/lodash".to_string(),
        json!({
            "version": "4.17.21",
            "integrity": valid_sri,
            "dependencies": {
                "ms": "1.2.3 secret-package",
            },
        }),
    );
    packages.insert(
        "node_modules/ms".to_string(),
        json!({
            "version": "2.1.3",
            "integrity": "https://user:secret-token@corp.internal/ms.tgz",
        }),
    );
    for idx in 0..3_000usize {
        packages.insert(
            format!("workspaces/w{idx}/node_modules/lodash"),
            json!({ "version": format!("0.0.{idx}") }),
        );
    }

    let payload: Map<String, Value> = serde_json::from_value(json!({
        "name": "app",
        "lockfileVersion": "secret-lockfile-string",
        "requires": {
            "lodash": "4.17.21 secret-package",
            "ms": "^2.1.3",
        },
        "dependencies": {
            "lodash": {
                "version": "4.17.21",
                "integrity": valid_sri,
                "requires": { "ms": "2.1.3 secret-package" },
            },
            "ms": {
                "version": "2.1.3",
                "integrity": "https://user:secret-token@corp.internal/ms.tgz",
            },
        },
        "packages": packages,
        "install": [
            "lodash@4.17.21 secret-package",
            "ms@^2.1.3",
        ],
    }))
    .unwrap();

    let destinations = HashMap::from([
        ("lodash".to_string(), PackageDestination::Upstream("npmjs".to_string())),
        ("ms".to_string(), PackageDestination::Upstream("npmjs".to_string())),
    ]);

    let sanitized = build_upstream_legacy_payload(&payload, "npmjs", &destinations);
    let serialized = serde_json::to_string(&sanitized).unwrap();
    assert!(!serialized.contains("secret-package"));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("secret-lockfile-string"));
    assert!(!serialized.contains("corp.internal"));
    assert_eq!(sanitized["dependencies"]["lodash"]["integrity"], valid_sri);
    assert!(sanitized["dependencies"]["ms"].get("integrity").is_none());
    assert_eq!(sanitized["packages"]["node_modules/lodash"]["integrity"], valid_sri);
    assert!(sanitized["packages"]["node_modules/ms"].get("integrity").is_none());
    assert_eq!(sanitized["install"], json!(["ms@^2.1.3"]));
    assert_eq!(
        sanitized["packages"]
            .as_object()
            .unwrap()
            .len(),
        3_003,
    );
}

#[test]
fn infer_patched_versions_and_ghsa_extraction_cover_common_ranges() {
    assert_eq!(infer_patched_versions("<2.0.0"), ">=2.0.0");
    assert_eq!(infer_patched_versions(">=1.0.0 <=1.2.3"), ">=1.2.4");
    assert_eq!(infer_patched_versions("<1.0.0 || >=2.0.0 <2.4.1"), ">=2.4.1");
    assert_eq!(infer_patched_versions(">=2.0.0 <2.1.0 || >=1.0.0 <1.5.0"), ">=2.1.0");
    assert_eq!(infer_patched_versions(">=1.0.0 <1.5.0 || >=2.0.0"), "<0.0.0");
    assert_eq!(infer_patched_versions("1.0.2 || 1.0.0"), ">=1.0.3");
    assert_eq!(infer_patched_versions("*"), "<0.0.0");
    assert_eq!(infer_patched_versions(">=1.0.0"), "<0.0.0");
    assert_eq!(
        derive_ghsa_id("https://github.com/advisories/GHSA-AbCd-1234-EfGh"),
        "GHSA-abcd-1234-efgh",
    );
    assert_eq!(derive_ghsa_id("https://example.com/CVE-2026-0001"), "");
    assert_eq!(parse_npm_alias("npm:@acme/pkg@1.2.3"), Some(("@acme/pkg", Some("1.2.3"))));
    assert_eq!(parse_name_and_spec("@acme/pkg@^1.0.0"), Some(("@acme/pkg", Some("^1.0.0"))));
}

#[test]
fn osv_legacy_report_and_upstream_response_sanitization_work() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("GHSA-unit-1111.json"),
        json!({
            "id": "GHSA-unit-1111",
            "summary": "Prototype pollution",
            "database_specific": { "severity": "CRITICAL", "cwe_ids": ["CWE-1321"] },
            "affected": [{
                "package": { "ecosystem": "npm", "name": "@acme/private-pkg" },
                "ranges": [{
                    "type": "SEMVER",
                    "events": [{ "introduced": "0" }, { "fixed": "1.5.0" }]
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();
    let index = OsvIndex::load_from_path(dir.path()).unwrap();
    let payload: Map<String, Value> = serde_json::from_value(json!({
        "dependencies": {
            "@acme/private-pkg": { "version": "1.2.0" },
            "safe-dev": { "version": "3.0.0", "dev": true }
        }
    }))
    .unwrap();
    let nodes = collect_installed_nodes(&payload).unwrap();
    let report = build_osv_legacy_report(&index, &nodes);
    assert_eq!(report.metadata.dependencies, 1);
    assert_eq!(report.metadata.dev_dependencies, 1);
    assert_eq!(report.metadata.total_dependencies, 2);
    assert_eq!(report.metadata.vulnerabilities.critical, 1);
    assert_eq!(report.metadata.vulnerabilities.total, 1);
    assert_eq!(report.advisories.len(), 1);
    let advisory = report.advisories
        .values()
        .next()
        .unwrap();
    assert_eq!(advisory.module_name, "@acme/private-pkg");
    assert_eq!(advisory.patched_versions, ">=1.5.0");
    assert_eq!(advisory.github_advisory_id, "GHSA-unit-1111");

    let mut spoofed_doc: Map<String, Value> = serde_json::from_value(json!({
        "actions": [
            { "module": "lodash", "action": "update" },
            { "module": "@acme/private-pkg", "action": "update" }
        ],
        "advisories": {
            "1": { "module_name": "lodash", "severity": "high" },
            "2": { "module_name": "@acme/private-pkg", "severity": "critical" }
        },
        "metadata": {
            "vulnerabilities": { "high": 1, "critical": 1, "total": 2 },
            "totalDependencies": 1
        }
    }))
    .unwrap();
    let allowed = HashSet::from(["lodash".to_string()]);
    sanitize_upstream_legacy_response(&mut spoofed_doc, &allowed);
    assert_eq!(
        spoofed_doc["advisories"]
            .as_object()
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(spoofed_doc["metadata"]["vulnerabilities"]["critical"], 0);
    assert_eq!(spoofed_doc["metadata"]["vulnerabilities"]["high"], 1);
    assert_eq!(spoofed_doc["metadata"]["vulnerabilities"]["total"], 1);

    let partner_doc: Map<String, Value> = serde_json::from_value(json!({
        "advisories": {
            "1": { "module_name": "@partner/sdk", "severity": "moderate" }
        },
        "metadata": { "dependencies": 1, "totalDependencies": 1 }
    }))
    .unwrap();
    let merged = merge_upstream_legacy_responses(vec![
        (allowed, spoofed_doc),
        (HashSet::from(["@partner/sdk".to_string()]), partner_doc),
    ]);
    assert_eq!(
        merged["advisories"]
            .as_object()
            .unwrap()
            .len(),
        2,
    );
    assert_eq!(merged["metadata"]["vulnerabilities"]["high"], 1);
    assert_eq!(merged["metadata"]["vulnerabilities"]["moderate"], 1);
    assert_eq!(merged["metadata"]["vulnerabilities"]["total"], 2);
    assert_eq!(merged["metadata"]["totalDependencies"], 2);
}
