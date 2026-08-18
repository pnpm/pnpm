use std::collections::BTreeMap;

use pnpm_config::{ResolutionMode, TrustPolicy};
use serde_json::json;

use super::{
    Frame, PnprClient, PnprClientError, ResolveProject, ResolveProjectsOptions, VerifyError,
    build_verify_error, parse_frame,
};

/// The request body is the whole contract with the server: a field the
/// client omits is not defaulted server-side but cleared, so the server
/// resolves under a policy the user never configured and cannot see
/// `catalog:` definitions at all. Assert on what actually goes over the
/// wire rather than on the options struct.
#[tokio::test]
async fn the_resolve_request_carries_the_catalogs_and_the_whole_policy() {
    let mut server = mockito::Server::new_async().await;
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(json!({
            "catalogs": { "default": { "acme": "^1.0.0" } },
            "autoInstallPeers": false,
            "dedupePeers": true,
            "excludeLinksFromLockfile": false,
            "resolutionMode": "time-based",
            "minimumReleaseAge": 1440,
            "minimumReleaseAgeExclude": ["@acme/*"],
            "minimumReleaseAgeIgnoreMissingTime": false,
            "trustPolicy": "no-downgrade",
            "trustPolicyExclude": ["legacy-pkg"],
            "trustPolicyIgnoreAfter": 43200,
            "trustLockfile": true,
        })))
        .with_body("{\"type\":\"done\",\"lockfile\":{\"lockfileVersion\":\"9.0\"}}\n")
        .create_async()
        .await;

    let _outcome = PnprClient::new(server.url())
        .resolve_projects(resolve_projects_options())
        .await
        .expect("the resolve succeeds");

    resolve_mock.assert_async().await;
}

fn resolve_projects_options() -> ResolveProjectsOptions {
    ResolveProjectsOptions {
        projects: vec![ResolveProject {
            dir: ".".to_string(),
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
            dependencies: BTreeMap::from([("acme".to_string(), "catalog:".to_string())]),
            dev_dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
        }],
        registry: "https://registry.test/".to_string(),
        registries: BTreeMap::new(),
        authorization: None,
        overrides: None,
        catalogs: Some(BTreeMap::from([(
            "default".to_string(),
            BTreeMap::from([("acme".to_string(), "^1.0.0".to_string())]),
        )])),
        auto_install_peers: Some(false),
        dedupe_peers: Some(true),
        exclude_links_from_lockfile: Some(false),
        lockfile: None,
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        trust_lockfile: true,
        resolution_mode: ResolutionMode::TimeBased,
        minimum_release_age: Some(1440),
        minimum_release_age_exclude: Some(vec!["@acme/*".to_string()]),
        minimum_release_age_ignore_missing_time: false,
        trust_policy: TrustPolicy::NoDowngrade,
        trust_policy_exclude: Some(vec!["legacy-pkg".to_string()]),
        trust_policy_ignore_after: Some(43200),
    }
}

#[test]
fn a_violations_frame_rebuilds_a_verify_error() {
    let line = br#"{"type":"violations","violations":[{"name":"@foo/no-deps","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"was published yesterday"}]}"#;
    let Frame::Violations { violations } = parse_frame(line).expect("frame parses") else {
        panic!("expected a violations frame");
    };
    let verify_err = build_verify_error(violations);
    assert!(
        matches!(verify_err, VerifyError::MinimumReleaseAgeViolation { .. }),
        "got {verify_err:?}",
    );
    assert!(verify_err.to_string().contains("@foo/no-deps@1.0.0"), "got {verify_err}");
}

#[test]
fn tarball_mismatch_maps_to_the_generic_envelope() {
    let line = br#"{"type":"violations","violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#;
    let Frame::Violations { violations } = parse_frame(line).expect("frame parses") else {
        panic!("expected a violations frame");
    };
    let verify_err = build_verify_error(violations);
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

#[test]
fn a_package_frame_parses_its_fetch_hint() {
    let line = br#"{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz","unpackedSize":123456,"fileCount":42}"#;
    let Frame::Package { id, name, version, integrity, tarball, unpacked_size, file_count } =
        parse_frame(line).expect("frame parses")
    else {
        panic!("expected a package frame");
    };
    assert_eq!(id, "acme@1.0.0");
    assert_eq!(name, "acme");
    assert_eq!(version, "1.0.0");
    assert_eq!(integrity, "sha512-abc");
    assert_eq!(tarball, "https://r.test/acme/-/acme-1.0.0.tgz");
    assert_eq!(unpacked_size, Some(123456));
    assert_eq!(file_count, Some(42));
}

#[test]
fn a_package_frame_without_dist_stats_parses() {
    let line = br#"{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz"}"#;
    let Frame::Package { unpacked_size, file_count, .. } = parse_frame(line).expect("frame parses")
    else {
        panic!("expected a package frame");
    };
    assert_eq!(unpacked_size, None);
    assert_eq!(file_count, None);
}

#[test]
fn an_untyped_frame_is_a_protocol_error() {
    let Err(PnprClientError::Protocol(_)) = parse_frame(b"{}") else {
        panic!("expected a Protocol error");
    };
}
