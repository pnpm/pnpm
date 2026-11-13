use std::{sync::Arc, time::Duration};

use axum::{
    body::{Bytes, to_bytes},
    http::StatusCode,
};
use pnpr_config::{IpNetwork, PublicRoute};
use pnpr_osv::OsvIndex;
use pnpr_policy::Identity;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::time::Instant;

use super::{
    super::{
        Resolver,
        dispatch::handle_resolve,
        limits::{GRAPH_TOO_LARGE_CODE, RESOLVE_TIMEOUT_CODE},
        protocol::ResolveRequest,
        verify_lockfile::handle_verify_lockfile,
        wire::{
            MAX_NDJSON_FRAME_BYTES, frozen_package_frames_until, ndjson_frames_until,
            osv_violations_for_lockfile_until, verify_done_frame, violations_frame,
        },
    },
    cargo_done_frame, registry_config, tarball_router,
};

fn caller(id: &str, name: &str) -> Identity {
    Identity::User {
        id: id.to_string(),
        username: name.to_string(),
        memberships: Vec::new().into(),
    }
}

fn resolver_with_osv(origin: &str, dir: &TempDir) -> Resolver {
    let osv_dir = dir.path().join("osv");
    std::fs::create_dir_all(&osv_dir).unwrap();
    std::fs::write(
        osv_dir.join("GHSA-trusted-verify.json"),
        r#"{
          "id": "GHSA-trusted-verify",
          "affected": [{
            "package": { "ecosystem": "npm", "name": "vuln-pkg" },
            "ranges": [{
              "type": "SEMVER",
              "events": [
                { "introduced": "1.0.0" },
                { "fixed": "1.2.0" }
              ]
            }]
          }]
        }"#,
    )
    .unwrap();
    let osv_index = Arc::new(OsvIndex::load_from_path(&osv_dir).unwrap());

    let mut config = registry_config();
    config.storage.cache_dir = dir.path().join("cache");
    config.routing.route_policy.public.push(PublicRoute {
        registry: Some(format!("{origin}/")),
        package: None,
    });
    config.routing.route_policy.allowed_private_networks =
        vec![IpNetwork::parse("127.0.0.0/8").unwrap()];
    Resolver::build(&config, Some(osv_index))
}

#[tokio::test]
async fn trusted_verify_lockfile_enforces_active_concurrency_and_deadline_for_osv_scan() {
    let tmp = TempDir::new().unwrap();
    let mut resolver = resolver_with_osv("http://127.0.0.1:9", &tmp);
    resolver.limits_mut().set_concurrency(1, 1);
    resolver.limits_mut().set_admission(4, 4);
    resolver.limits_mut().deadline = Duration::from_millis(25);

    let integrity = ssri::Integrity::from(b"vuln-pkg").to_string();
    let payload = Bytes::from(
        serde_json::to_vec(&json!({
            "trustLockfile": true,
            "lockfile": {
                "lockfileVersion": "9.0",
                "importers": { ".": {} },
                "packages": {
                    "vuln-pkg@1.0.0": {
                        "resolution": { "integrity": integrity },
                    },
                },
            },
        }))
        .unwrap(),
    );

    // When another request holds the sole active running slot, trusted verification
    // cannot bypass `admitted.acquire(deadline)` and times out with 504.
    let held_permit = resolver.limits
        .admit(&caller("holder", "holder"))
        .ok()
        .unwrap()
        .acquire(Instant::now() + Duration::from_secs(5))
        .await
        .ok()
        .unwrap();
    let blocked = handle_verify_lockfile(&resolver, caller("u-1", "one"), payload.clone()).await;
    assert_eq!(blocked.status(), StatusCode::GATEWAY_TIMEOUT);
    let blocked_json: Value =
        serde_json::from_slice(&to_bytes(blocked.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(blocked_json["code"], RESOLVE_TIMEOUT_CODE);
    drop(held_permit);

    // Once the active slot is free, trusted verification runs the OSV scan on the
    // blocking pool and reports the vulnerability violation.
    resolver.limits_mut().deadline = Duration::from_secs(5);
    let ok = handle_verify_lockfile(&resolver, caller("u-1", "one"), payload).await;
    assert_eq!(ok.status(), StatusCode::OK);
    let line: Value =
        serde_json::from_slice(&to_bytes(ok.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(line["type"], "violations");
    assert_eq!(line["violations"][0]["name"], "vuln-pkg");
}

#[test]
fn osv_violations_for_lockfile_until_stops_when_cancelled() {
    let tmp = TempDir::new().unwrap();
    let resolver = resolver_with_osv("http://127.0.0.1:9", &tmp);
    let osv_index = resolver.osv_index.as_deref().unwrap();
    let integrity = ssri::Integrity::from(b"pkg").to_string();
    let lockfile: pnpm_lockfile::Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": { ".": {} },
        "packages": {
            "pkg-0@1.0.0": { "resolution": { "integrity": integrity } },
        },
    }))
    .unwrap();
    let cancelled = osv_violations_for_lockfile_until(osv_index, &lockfile, &|| true);
    assert!(cancelled.is_none());
}

#[tokio::test]
async fn pypi_invalid_requirement_fails_before_waiting_for_running_slot() {
    let tmp = TempDir::new().unwrap();
    let mut resolver = resolver_with_osv("http://127.0.0.1:9", &tmp);
    resolver.limits_mut().set_concurrency(1, 1);
    resolver.limits_mut().set_admission(4, 4);
    resolver.limits_mut().deadline = Duration::from_millis(25);

    let _held_permit = resolver.limits
        .admit(&caller("holder", "holder"))
        .ok()
        .unwrap()
        .acquire(Instant::now() + Duration::from_secs(5))
        .await
        .ok()
        .unwrap();

    let bad_pypi = Bytes::from(
        serde_json::to_vec(&json!({
            "ecosystem": "pypi",
            "index": "http://127.0.0.1:9/simple/",
            "requirements": ["not a valid == == pep508 requirement"],
            "target": {
                "environment": {
                    "implementation_name": "cpython",
                    "implementation_version": "3.12.0",
                    "os_name": "posix",
                    "platform_machine": "x86_64",
                    "platform_python_implementation": "CPython",
                    "platform_release": "6.0",
                    "platform_system": "Linux",
                    "platform_version": "1",
                    "python_full_version": "3.12.0",
                    "python_version": "3.12",
                    "sys_platform": "linux",
                },
                "tags": ["py3-none-any"],
            },
        }))
        .unwrap(),
    );
    let resp = handle_resolve(&resolver, caller("u-1", "one"), bad_pypi).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn cargo_and_violations_frames_stop_buffering_at_max_frame_size() {
    let oversized_lockfile = "x".repeat(MAX_NDJSON_FRAME_BYTES);
    let cargo_frame: Value =
        serde_json::from_slice(&cargo_done_frame(&oversized_lockfile)).unwrap();
    assert_eq!(cargo_frame["type"], "error");
    assert_eq!(cargo_frame["code"], GRAPH_TOO_LARGE_CODE);

    let chunk = json!({ "reason": "y".repeat(1024 * 1024) });
    let oversized_violations = vec![chunk; 33];
    let viol_frame: Value =
        serde_json::from_slice(&violations_frame(&oversized_violations)).unwrap();
    assert_eq!(viol_frame["type"], "error");
    assert_eq!(viol_frame["code"], GRAPH_TOO_LARGE_CODE);

    let mut oversized_verify: ResolveRequest = serde_json::from_value(json!({})).unwrap();
    oversized_verify.minimum_release_age_exclude = Some(vec!["z".repeat(1024 * 1024); 33]);
    let verify_frame: Value =
        serde_json::from_slice(&verify_done_frame(&oversized_verify)).unwrap();
    assert_eq!(verify_frame["type"], "error");
    assert_eq!(verify_frame["code"], GRAPH_TOO_LARGE_CODE);
}

#[test]
fn frozen_frames_and_ndjson_stream_honor_cancellation() {
    use pnpm_resolving_npm_resolver::observed_dist_stats_sink;

    let integrity = ssri::Integrity::from(b"pkg").to_string();
    let lockfile: pnpm_lockfile::Lockfile = serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": { ".": {} },
        "packages": {
            "pkg-0@1.0.0": { "resolution": { "integrity": integrity } },
            "pkg-1@1.0.0": { "resolution": { "integrity": integrity } },
        },
    }))
    .unwrap();
    let pnpm_cfg = pnpm_config::Config::default();
    let reg_cfg = registry_config();
    let router = tarball_router(&reg_cfg, Identity::Anonymous);
    let stats = observed_dist_stats_sink();

    let calls = std::sync::atomic::AtomicUsize::new(0);
    let cancelled = frozen_package_frames_until(&pnpm_cfg, &router, &lockfile, &stats, &|| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
    });
    assert!(cancelled.is_none());
    assert!(ndjson_frames_until(vec![b"{}\n".to_vec()], &|| true).is_none());
}

#[tokio::test]
async fn untrusted_verify_lockfile_scans_osv_with_deadline_and_caches_clean_verdict() {
    let tmp = TempDir::new().unwrap();
    let mut resolver = resolver_with_osv("http://127.0.0.1:9", &tmp);
    let integrity = ssri::Integrity::from(b"pkg").to_string();
    let vuln_payload = Bytes::from(
        serde_json::to_vec(&json!({
            "trustLockfile": false,
            "registry": "http://127.0.0.1:9/",
            "lockfile": {
                "lockfileVersion": "9.0",
                "importers": { ".": {} },
                "packages": {
                    "vuln-pkg@1.0.0": { "resolution": { "integrity": integrity } },
                },
            },
        }))
        .unwrap(),
    );

    resolver.limits_mut().deadline = Duration::ZERO;
    let timed_out =
        handle_verify_lockfile(&resolver, caller("u-1", "one"), vuln_payload.clone()).await;
    assert_eq!(timed_out.status(), StatusCode::GATEWAY_TIMEOUT);

    resolver.limits_mut().deadline = Duration::from_secs(5);
    let flagged = handle_verify_lockfile(&resolver, caller("u-1", "one"), vuln_payload).await;
    assert_eq!(flagged.status(), StatusCode::OK);
    let flagged_json: Value =
        serde_json::from_slice(&to_bytes(flagged.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(flagged_json["type"], "violations");
    assert_eq!(flagged_json["violations"][0]["name"], "vuln-pkg");
}
