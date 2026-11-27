use std::sync::Arc;

use axum::{
    body::{
        Bytes,
        to_bytes,
    },
    http::StatusCode,
    response::Response,
};
use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::with_offline_check_cache_identities;
use pnpm_package_manager::build_resolution_verifiers;
use pnpr_config::{
    IpNetwork,
    PublicRoute,
};
use pnpr_route::Footprint;
use serde_json::{
    Value,
    json,
};
use tempfile::TempDir;

use super::{
    super::{
        Resolver,
        dispatch::handle_resolve,
        request_tarball_router,
        verdict_cache::{
            VerdictLookup,
            VerdictRecord,
        },
        verify_lockfile::{
            cached_verdict::{
                merge_policies,
                past_verdict_footprint,
            },
            handle_verify_lockfile,
        },
    },
    registry_config,
    user,
};

fn test_resolver(origin: &str, dir: &TempDir) -> Resolver {
    let mut config = registry_config();
    config.storage.cache_dir = dir.path().to_path_buf();
    config.http.public_url = Some("http://example.test".into());
    config.routing.route_policy.public.push(PublicRoute {
        registry: Some(format!("{origin}/")),
        package: None,
    });
    config.routing.route_policy.allowed_private_networks =
        vec![IpNetwork::parse("127.0.0.0/8").unwrap()];
    Resolver::build(&config, None)
}

fn shape_mismatched_lockfile(resolution: &Value) -> Lockfile {
    serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": {
                "dependencies": {
                    "safe-pkg": {
                        "specifier": "1.0.0",
                        "version": "1.0.0",
                    }
                }
            }
        },
        "packages": {
            "safe-pkg@1.0.0": {
                "resolution": resolution,
            }
        },
        "snapshots": {
            "safe-pkg@1.0.0": {}
        }
    }))
    .expect("fixture lockfile parses")
}

async fn parse_single_ndjson_frame(response: Response) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("read body");
    let text = std::str::from_utf8(&bytes).expect("utf8 body");
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "expected a single terminal NDJSON frame, got: {text}");
    serde_json::from_str(lines[0]).expect("frame JSON parses")
}

fn assert_shape_mismatch_frame(frame: &Value) {
    assert_eq!(frame["type"], "violations", "expected violations frame, got: {frame}");
    let violations = frame["violations"].as_array().expect("violations array");
    assert_eq!(violations.len(), 1, "frame: {frame}");
    assert_eq!(violations[0]["name"], "safe-pkg");
    assert_eq!(violations[0]["version"], "1.0.0");
    assert_eq!(violations[0]["code"], "RESOLUTION_SHAPE_MISMATCH");
}

async fn assert_shape_mismatch_for_trust_mode(
    runtime: &Resolver,
    identity: &pnpr_policy::Identity,
    origin: &str,
    label: &str,
    lockfile: &Lockfile,
    trust_lockfile: bool,
) {
    let verify_body = Bytes::from(
        json!({
            "registry": format!("{origin}/"),
            "trustLockfile": trust_lockfile,
            "lockfile": lockfile,
        })
        .to_string(),
    );
    let verify_frame = parse_single_ndjson_frame(
        handle_verify_lockfile(runtime, identity.clone(), verify_body).await,
    )
    .await;
    assert_shape_mismatch_frame(&verify_frame);

    for frozen_lockfile in [true, false] {
        let resolve_body = Bytes::from(
            json!({
                "dependencies": { "safe-pkg": "1.0.0" },
                "registry": format!("{origin}/"),
                "frozenLockfile": frozen_lockfile,
                "trustLockfile": trust_lockfile,
                "lockfile": lockfile,
            })
            .to_string(),
        );
        let resolve_frame = parse_single_ndjson_frame(
            handle_resolve(runtime, identity.clone(), resolve_body).await,
        )
        .await;
        assert_shape_mismatch_frame(&resolve_frame);
        let epoch = runtime.local_registry.verdict_epoch().expect("stable epoch");
        let pacquet_config = super::config_for_registry(&format!("{origin}/"));
        let hash = request_tarball_router(runtime, identity, &pacquet_config)
            .verification_verdict_hash(&pacquet_config, lockfile);
        let is_verified = runtime.cache.verdicts
            .as_ref()
            .is_some_and(|cache| {
                cache
                    .verified_footprint(
                        &VerdictLookup {
                            hash: &hash,
                            epoch: &epoch,
                            secret: &runtime.cache.secret,
                            route_context: &runtime.route_context,
                            identity,
                        },
                        |_| true,
                    )
                    .is_some()
            });
        assert!(
            !is_verified,
            "{label} (frozen={frozen_lockfile}, trust={trust_lockfile}) must not record a verdict",
        );
    }
}

async fn assert_shape_mismatch_rejected(
    runtime: &Resolver,
    identity: &pnpr_policy::Identity,
    origin: &str,
    label: &str,
    resolution: &Value,
) {
    let lockfile = shape_mismatched_lockfile(resolution);
    for trust_lockfile in [false, true] {
        assert_shape_mismatch_for_trust_mode(
            runtime,
            identity,
            origin,
            label,
            &lockfile,
            trust_lockfile,
        )
        .await;
    }
}

#[tokio::test]
async fn verify_lockfile_and_resolve_reject_shape_mismatched_entries() {
    let dir = TempDir::new().expect("tempdir");
    let origin = "http://127.0.0.1:9";
    let runtime = test_resolver(origin, &dir);
    let identity = user("alice");

    let cases = [
        (
            "file-tarball",
            json!({
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "tarball": "file:alt.tgz",
            }),
        ),
        (
            "directory",
            json!({
                "type": "directory",
                "directory": "../alt",
            }),
        ),
        (
            "git",
            json!({
                "type": "git",
                "repo": "https://example.com/safe-pkg.git",
                "commit": "0123456789abcdef0123456789abcdef01234567",
            }),
        ),
    ];

    for (label, resolution) in &cases {
        assert_shape_mismatch_rejected(&runtime, &identity, origin, label, resolution).await;
    }
}

#[tokio::test]
async fn verdict_cache_requires_shape_check_policy_and_never_bypasses_shape_mismatch() {
    let dir = TempDir::new().expect("tempdir");
    let origin = "http://127.0.0.1:9";
    let runtime = test_resolver(origin, &dir);
    let identity = user("alice");

    let mut pacquet_config = super::config_for_registry(&format!("{origin}/"));
    pacquet_config.minimum_release_age = Some(0);
    let verifiers = build_resolution_verifiers(
        &pacquet_config,
        Arc::clone(&runtime.client),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("build verifiers");
    let cache_verifiers = with_offline_check_cache_identities(&verifiers);

    let clean_lockfile = super::package_lockfile("safe-pkg", "1.0.0");
    let router = request_tarball_router(&runtime, &identity, &pacquet_config);
    let clean_hash = router.verification_verdict_hash(&pacquet_config, &clean_lockfile);
    let epoch = runtime.local_registry.verdict_epoch().expect("stable epoch");
    let footprint = Footprint::default();
    let verdicts = runtime.cache.verdicts.as_ref().expect("verdict cache open");

    // Verdict rows lacking `resolutionShapeCheck: 2` (absent or boolean `true`) miss.
    verdicts.record(VerdictRecord {
        hash: &clean_hash,
        footprint: &footprint,
        secret: &runtime.cache.secret,
        epoch: &epoch,
        policy: merge_policies(&verifiers, None),
    });
    assert!(
        past_verdict_footprint(&runtime, &identity, &clean_hash, Some(&epoch), &cache_verifiers)
            .is_none(),
    );
    let mut bool_shape_policy = merge_policies(&cache_verifiers, None);
    bool_shape_policy["resolutionShapeCheck"] = json!(true);
    verdicts.record(VerdictRecord {
        hash: &clean_hash,
        footprint: &footprint,
        secret: &runtime.cache.secret,
        epoch: &epoch,
        policy: bool_shape_policy,
    });
    assert!(
        past_verdict_footprint(&runtime, &identity, &clean_hash, Some(&epoch), &cache_verifiers)
            .is_none(),
    );

    // Merged policy carrying `resolutionShapeCheck: 2` hits for a clean lockfile.
    verdicts.record(VerdictRecord {
        hash: &clean_hash,
        footprint: &footprint,
        secret: &runtime.cache.secret,
        epoch: &epoch,
        policy: merge_policies(&cache_verifiers, None),
    });
    assert_eq!(
        past_verdict_footprint(&runtime, &identity, &clean_hash, Some(&epoch), &cache_verifiers),
        Some(footprint.clone()),
    );

    // Even if a tampered lockfile's hash were pre-seeded with the full policy in SQLite,
    // `verify-lockfile` and `resolve` still reject it on the offline shape check.
    let poisoned = shape_mismatched_lockfile(&json!({
        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        "tarball": "file:alt.tgz",
    }));
    let poisoned_hash = router.verification_verdict_hash(&pacquet_config, &poisoned);
    verdicts.record(VerdictRecord {
        hash: &poisoned_hash,
        footprint: &footprint,
        secret: &runtime.cache.secret,
        epoch: &epoch,
        policy: merge_policies(&cache_verifiers, None),
    });

    let verify_body = Bytes::from(
        json!({
            "registry": format!("{origin}/"),
            "lockfile": poisoned,
        })
        .to_string(),
    );
    let verify_frame = parse_single_ndjson_frame(
        handle_verify_lockfile(&runtime, identity.clone(), verify_body).await,
    )
    .await;
    assert_shape_mismatch_frame(&verify_frame);

    let resolve_body = Bytes::from(
        json!({
            "dependencies": { "safe-pkg": "1.0.0" },
            "registry": format!("{origin}/"),
            "frozenLockfile": true,
            "lockfile": poisoned,
        })
        .to_string(),
    );
    let resolve_frame =
        parse_single_ndjson_frame(handle_resolve(&runtime, identity, resolve_body).await).await;
    assert_shape_mismatch_frame(&resolve_frame);
}

#[tokio::test]
async fn verify_lockfile_and_resolve_reject_invalid_lockfile_dependency_names() {
    let dir = TempDir::new().expect("tempdir");
    let origin = "http://127.0.0.1:9";
    let runtime = test_resolver(origin, &dir);
    let identity = user("alice");

    let importer_traversal: Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": {
                "dependencies": {
                    "../../../escape": {
                        "specifier": "1.0.0",
                        "version": "1.0.0",
                    }
                }
            }
        },
        "packages": {},
        "snapshots": {},
    }))
    .expect("importer traversal lockfile parses");
    let packages_traversal: Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": { ".": {} },
        "packages": {
            "../../../escape@1.0.0": {
                "resolution": { "integrity": "sha512-deadbeef" },
            }
        },
        "snapshots": {},
    }))
    .expect("packages traversal lockfile parses");

    for lockfile in [&importer_traversal, &packages_traversal] {
        assert_invalid_dependency_name_rejected(&runtime, &identity, origin, lockfile).await;
    }
}

async fn assert_invalid_dependency_name_rejected(
    runtime: &Resolver,
    identity: &pnpr_policy::Identity,
    origin: &str,
    lockfile: &Lockfile,
) {
    for trust_lockfile in [false, true] {
        let verify_body = Bytes::from(
            json!({
                "registry": format!("{origin}/"),
                "trustLockfile": trust_lockfile,
                "lockfile": lockfile,
            })
            .to_string(),
        );
        let verify_resp = handle_verify_lockfile(runtime, identity.clone(), verify_body).await;
        assert_eq!(verify_resp.status(), StatusCode::BAD_REQUEST);
        let verify_bytes = to_bytes(verify_resp.into_body(), 64 * 1024).await.expect("read body");
        let verify_json: Value = serde_json::from_slice(&verify_bytes).expect("parse json");
        assert_eq!(verify_json["code"], "ERR_PNPR_INVALID_DEPENDENCY_NAME");

        let resolve_body = Bytes::from(
            json!({
                "dependencies": {},
                "registry": format!("{origin}/"),
                "trustLockfile": trust_lockfile,
                "lockfile": lockfile,
            })
            .to_string(),
        );
        let resolve_resp = handle_resolve(runtime, identity.clone(), resolve_body).await;
        assert_eq!(resolve_resp.status(), StatusCode::BAD_REQUEST);
        let resolve_bytes = to_bytes(resolve_resp.into_body(), 64 * 1024).await.expect("read body");
        let resolve_json: Value = serde_json::from_slice(&resolve_bytes).expect("parse json");
        assert_eq!(resolve_json["code"], "ERR_PNPR_INVALID_DEPENDENCY_NAME");
    }
}
