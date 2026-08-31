use std::{collections::BTreeMap, future::pending, time::Duration};

use indexmap::IndexMap;
use pnpm_config::{PackageExtension, ResolutionMode, TrustPolicy};
use pnpm_graph_hasher::hash_object_nullable_with_prefix;
use pnpm_lockfile::TarballRevision;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::{
    Frame, PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION, PnprClient, PnprClientError,
    ResolveOutcome, ResolveProject, ResolveProjectsOptions, ResolvedPackage, VerifyError,
    build_verify_error, parse_frame,
};

#[tokio::test]
async fn artifact_handshake_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (_connection, _) = listener.accept().await.unwrap();
        pending::<()>().await;
    });
    let mut client = PnprClient::new(format!("http://{address}"));
    client.artifact_request_timeout = Duration::from_millis(25);

    let error = client.handshake_artifacts().await.unwrap_err();

    assert!(matches!(error, PnprClientError::Http(error) if error.is_timeout()));
    server.abort();
}

#[tokio::test]
async fn lockfile_repair_rejects_a_server_without_the_capability() {
    let mut options = resolve_projects_options();
    options.fix_lockfile = true;
    options.update_patches = false;
    let mut server = mockito::Server::new_async().await;
    let handshake_mock = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"versions":[0]}}"#)
        .create_async()
        .await;

    let Err(error) = PnprClient::new(server.url()).resolve_projects(options).await else {
        panic!("an older server must not silently ignore repair mode");
    };

    assert!(error.to_string().contains("does not advertise lockfile repair support"));
    handshake_mock.assert_async().await;
}

#[tokio::test]
async fn lockfile_repair_uses_an_advertised_capability() {
    let mut options = resolve_projects_options();
    options.fix_lockfile = true;
    options.update_patches = false;
    let response_lockfile = matching_transform_lockfile(&options);
    let mut server = mockito::Server::new_async().await;
    let handshake_mock = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"versions":[0],"fixLockfile":[0]}}"#)
        .create_async()
        .await;
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(json!({ "fixLockfile": true })))
        .with_header(PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION)
        .with_body(format!("{}\n", json!({ "type": "done", "lockfile": response_lockfile })))
        .create_async()
        .await;

    let _ = PnprClient::new(server.url())
        .resolve_projects(options)
        .await
        .expect("the advertised repair request succeeds");

    handshake_mock.assert_async().await;
    resolve_mock.assert_async().await;
}

/// The request body is the whole contract with the server: a field the
/// client omits is not defaulted server-side but cleared, so the server
/// resolves under a policy the user never configured and cannot see
/// `catalog:` definitions at all. Assert on what actually goes over the
/// wire rather than on the options struct.
#[tokio::test]
async fn the_resolve_request_carries_the_catalogs_and_the_whole_policy() {
    let options = resolve_projects_options();
    let response_lockfile = matching_transform_lockfile(&options);
    let mut server = mockito::Server::new_async().await;
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(json!({
            "catalogs": { "default": { "acme": "^1.0.0" } },
            "patchedDependencies": { "acme@1.0.0": "abc123" },
            "packageExtensions": {
                "acme@1.0.0": { "dependencies": { "helper": "1.0.0" } }
            },
            "allowUnusedPatches": true,
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
            "updatePatches": true,
        })))
        .with_header(PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION)
        .with_body(format!("{}\n", json!({ "type": "done", "lockfile": response_lockfile })))
        .create_async()
        .await;

    let _outcome = PnprClient::new(server.url())
        .resolve_projects(options)
        .await
        .expect("the resolve succeeds");

    resolve_mock.assert_async().await;
}

#[tokio::test]
async fn rejects_a_server_that_omits_or_changes_patch_metadata() {
    let options = resolve_projects_options();
    let extensions_checksum = package_extensions_checksum(&options);
    for patched_dependencies in [None, Some(json!({ "acme@1.0.0": "different-hash" }))] {
        let mut lockfile = json!({
            "lockfileVersion": "9.0",
            "packageExtensionsChecksum": extensions_checksum,
        });
        if let Some(patched_dependencies) = patched_dependencies {
            lockfile["patchedDependencies"] = patched_dependencies;
        }
        assert_transform_metadata_rejected(
            resolve_projects_options(),
            lockfile,
            "returned patchedDependencies that do not match the request",
        )
        .await;
    }
}

#[tokio::test]
async fn rejects_a_server_that_omits_or_changes_package_extension_metadata() {
    for package_extensions_checksum in [None, Some("sha256-different-checksum")] {
        let mut lockfile = json!({
            "lockfileVersion": "9.0",
            "patchedDependencies": { "acme@1.0.0": "abc123" },
        });
        if let Some(package_extensions_checksum) = package_extensions_checksum {
            lockfile["packageExtensionsChecksum"] = json!(package_extensions_checksum);
        }
        assert_transform_metadata_rejected(
            resolve_projects_options(),
            lockfile,
            "returned packageExtensionsChecksum that does not match the request",
        )
        .await;
    }
}

#[tokio::test]
async fn rejects_an_old_server_before_consuming_package_frames() {
    let frames = vec![
        mock_package_frame(),
        json!({
            "type": "done",
            "lockfile": { "lockfileVersion": "9.0" },
        }),
    ];
    let mut prefetched = Vec::new();
    let result = resolve_mock_frames(resolve_projects_options(), frames, false, |package| {
        prefetched.push(package.id);
    })
    .await;

    let Err(PnprClientError::Protocol(message)) = result else {
        panic!("expected a protocol error");
    };
    assert!(message.contains("does not advertise project-transform support"));
    assert!(prefetched.is_empty(), "an old server must not trigger prefetches");
}

#[tokio::test]
async fn a_project_transform_header_preserves_streaming_prefetch() {
    let frames =
        vec![mock_package_frame(), json!({ "type": "error", "message": "resolution stopped" })];
    let mut prefetched = Vec::new();
    let result = resolve_mock_frames(resolve_projects_options(), frames, true, |package| {
        prefetched.push(package.id);
    })
    .await;

    assert!(matches!(result, Err(PnprClientError::Server(_))));
    assert_eq!(prefetched, ["acme@1.0.0"]);
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
        patched_dependencies: Some(IndexMap::from([(
            "acme@1.0.0".to_string(),
            "abc123".to_string(),
        )])),
        package_extensions: Some(IndexMap::from([(
            "acme@1.0.0".to_string(),
            PackageExtension {
                dependencies: Some(BTreeMap::from([("helper".to_string(), "1.0.0".to_string())])),
                ..PackageExtension::default()
            },
        )])),
        allow_unused_patches: true,
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
        update_patches: true,
        fix_lockfile: false,
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

fn matching_transform_lockfile(options: &ResolveProjectsOptions) -> Value {
    json!({
        "lockfileVersion": "9.0",
        "patchedDependencies": options.patched_dependencies,
        "packageExtensionsChecksum": package_extensions_checksum(options),
    })
}

fn package_extensions_checksum(options: &ResolveProjectsOptions) -> String {
    let package_extensions = serde_json::to_value(
        options.package_extensions.as_ref().expect("package extensions are configured"),
    )
    .expect("package extensions serialize");
    hash_object_nullable_with_prefix(&package_extensions)
        .expect("configured package extensions have a checksum")
}

async fn assert_transform_metadata_rejected(
    options: ResolveProjectsOptions,
    lockfile: Value,
    expected_message: &str,
) {
    let Err(PnprClientError::Protocol(message)) = resolve_mock_frames(
        options,
        vec![json!({ "type": "done", "lockfile": lockfile })],
        true,
        |_| {},
    )
    .await
    else {
        panic!("expected a protocol error");
    };
    assert!(message.contains(expected_message), "got {message}");
}

async fn resolve_mock_frames(
    options: ResolveProjectsOptions,
    frames: Vec<Value>,
    supports_project_transforms: bool,
    on_package: impl FnMut(ResolvedPackage),
) -> Result<ResolveOutcome, PnprClientError> {
    let body = frames.into_iter().map(|frame| format!("{frame}\n")).collect::<Vec<_>>().concat();
    let mut server = mockito::Server::new_async().await;
    let mut resolve_mock = server.mock("POST", "/-/pnpr/v0/resolve").with_body(body);
    if supports_project_transforms {
        resolve_mock =
            resolve_mock.with_header(PROJECT_TRANSFORMS_HEADER, PROJECT_TRANSFORMS_VERSION);
    }
    let resolve_mock = resolve_mock.create_async().await;

    let result =
        PnprClient::new(server.url()).resolve_projects_streaming(options, on_package).await;
    resolve_mock.assert_async().await;
    result
}

fn mock_package_frame() -> Value {
    json!({
        "type": "package",
        "id": "acme@1.0.0",
        "name": "acme",
        "version": "1.0.0",
        "integrity": "sha512-abc",
        "tarball": "https://r.test/acme/-/acme-1.0.0.tgz",
    })
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
    let line = br#"{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz","unpackedSize":123456,"fileCount":42,"revision":3}"#;
    let Frame::Package {
        id,
        name,
        version,
        integrity,
        tarball,
        unpacked_size,
        file_count,
        revision,
    } = parse_frame(line).expect("frame parses")
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
    assert_eq!(revision, Some(TarballRevision::try_from(3).unwrap()));
}

#[test]
fn package_frames_reject_invalid_revisions() {
    for revision in [0, 9_007_199_254_740_992_u64] {
        let line = format!(
            r#"{{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz","revision":{revision}}}"#,
        );
        assert!(matches!(parse_frame(line.as_bytes()), Err(PnprClientError::Protocol(_)),));
    }
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
