mod line_limits;

use super::{
    PackageExtension, PnprClient, PnprClientError, ResolutionMode, ResolveOutcome, ResolveProject,
    ResolveProjectsOptions, ResolvedPackage, TrustPolicy, VerifyError, build_verify_error,
    resolve::{Frame, parse_frame},
};
use indexmap::IndexMap;
use pnpm_graph_hasher::hash_object_nullable_with_prefix;
use pnpm_lockfile::TarballRevision;
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::pending, time::Duration};
use tokio::net::TcpListener;

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
    options.reuse.update_patches = false;
    let mut server = mockito::Server::new_async().await;
    let handshake_mock = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"protocols":{"resolver":{"versions":[0],"features":["projectTransforms"]}}}}"#)
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
    options.reuse.update_patches = false;
    let response_lockfile = matching_transform_lockfile(&options);
    let mut server = mockito::Server::new_async().await;
    let handshake_mock = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"protocols":{"resolver":{"versions":[0],"features":["projectTransforms","fixLockfile"]}}}}"#)
        .create_async()
        .await;
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(json!({ "fixLockfile": true })))
        .with_body(format!("{}\n", json!({ "type": "done", "lockfileFormat": "pnpm-lock/9.0", "lockfile": response_lockfile, "appliedPolicies": matching_applied_policies(&options) })))
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
    let _handshake = mock_transform_handshake(&mut server, true).await;
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
            "addMissingPeerTypes": true,
            "projectScopedOptionalPeers": false,
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
        .with_body(format!("{}\n", json!({ "type": "done", "lockfileFormat": "pnpm-lock/9.0", "lockfile": response_lockfile, "appliedPolicies": matching_applied_policies(&options) })))
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
        assert_protocol_rejected(
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
        assert_protocol_rejected(
            resolve_projects_options(),
            lockfile,
            "returned packageExtensionsChecksum that does not match the request",
        )
        .await;
    }
}

#[tokio::test]
async fn rejects_a_server_that_omits_or_changes_a_project_publish_directory() {
    for publish_directory in [None, Some("build")] {
        let mut options = resolve_projects_options();
        options.projects[0].publish_config =
            Some(crate::PublishConfig { directory: "dist".to_string(), link_directory: None });
        let mut importer = json!({});
        if let Some(publish_directory) = publish_directory {
            importer["publishDirectory"] = json!(publish_directory);
        }
        let mut lockfile = matching_transform_lockfile(&options);
        lockfile["importers"] = json!({ ".": importer });
        assert_protocol_rejected(options, lockfile, "instead of its publishConfig.directory").await;
    }
}

#[tokio::test]
async fn does_not_check_a_project_the_server_did_not_return() {
    let mut options = resolve_projects_options();
    options.projects[0].publish_config =
        Some(crate::PublishConfig { directory: "dist".to_string(), link_directory: None });
    // A partial install resolves a subset of the workspace, so a project the
    // response leaves out carries no importer to compare a publish directory
    // against, and the merge only takes the importers the server did return.
    let lockfile = matching_transform_lockfile(&options);
    let applied = matching_applied_policies(&options);
    let result = resolve_mock_frames(
        options,
        vec![json!({ "type": "done", "lockfileFormat": "pnpm-lock/9.0", "lockfile": lockfile, "appliedPolicies": applied })],
        true,
        |_| {},
    )
    .await;

    let Ok(outcome) = result else {
        panic!("a response that omits the project must not fail the resolve");
    };
    assert!(
        outcome.lockfile.importers.is_empty(),
        "the resolve must not invent an importer for the omitted project",
    );
}

#[tokio::test]
async fn rejects_an_old_server_before_consuming_package_frames() {
    let frames = vec![
        mock_package_frame(),
        json!({
            "type": "done",
            "lockfileFormat": "pnpm-lock/9.0",
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
async fn advertised_project_transforms_preserve_streaming_prefetch() {
    let frames =
        vec![mock_package_frame(), json!({ "type": "error", "message": "resolution stopped" })];
    let mut prefetched = Vec::new();
    let result = resolve_mock_frames(resolve_projects_options(), frames, true, |package| {
        prefetched.push(package.id);
    })
    .await;

    assert!(matches!(result, Err(PnprClientError::Server { .. })));
    assert_eq!(prefetched, ["acme@1.0.0"]);
}

fn resolve_projects_options() -> ResolveProjectsOptions {
    ResolveProjectsOptions {
        projects: vec![ResolveProject {
            dir: ".".to_string(),
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
            publish_config: None,
            dependencies: BTreeMap::from([("acme".to_string(), "catalog:".to_string())]),
            dev_dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
            peer_dependencies: BTreeMap::new(),
        }],
        fix_lockfile: false,
        routing: crate::RegistryRouting {
            registry: "https://registry.test/".to_string(),
            registries: indexmap::IndexMap::new(),
            authorization: None,
        },
        transforms: crate::ManifestTransforms {
            overrides: None,
            patched_dependencies: Some(IndexMap::from([(
                "acme@1.0.0".to_string(),
                "abc123".to_string(),
            )])),
            package_extensions: Some(IndexMap::from([(
                "acme@1.0.0".to_string(),
                PackageExtension {
                    dependencies: Some(BTreeMap::from([(
                        "helper".to_string(),
                        "1.0.0".to_string(),
                    )])),
                    ..PackageExtension::default()
                },
            )])),
            allow_unused_patches: true,
            catalogs: Some(BTreeMap::from([(
                "default".to_string(),
                BTreeMap::from([("acme".to_string(), "^1.0.0".to_string())]),
            )])),
        },
        resolution: crate::ResolutionSettings {
            auto_install_peers: Some(false),
            dedupe_peers: Some(true),
            exclude_links_from_lockfile: Some(false),
            add_missing_peer_types: Some(true),
            project_scoped_optional_peers: Some(false),
            resolution_mode: ResolutionMode::TimeBased,
        },
        reuse: crate::LockfileReuseOptions {
            lockfile: None,
            frozen_lockfile: false,
            prefer_frozen_lockfile: None,
            update_patches: true,
            ignore_manifest_check: false,
            trust_lockfile: true,
        },
        verification: crate::VerificationPolicy {
            minimum_release_age: Some(1440),
            minimum_release_age_exclude: Some(vec!["@acme/*".to_string()]),
            minimum_release_age_ignore_missing_time: false,
            trust_policy: TrustPolicy::NoDowngrade,
            trust_policy_exclude: Some(vec!["legacy-pkg".to_string()]),
            trust_policy_ignore_after: Some(43200),
        },
    }
}

fn matching_transform_lockfile(options: &ResolveProjectsOptions) -> Value {
    json!({
        "lockfileVersion": "9.0",
        "patchedDependencies": options.transforms.patched_dependencies,
        "packageExtensionsChecksum": package_extensions_checksum(options),
    })
}

fn package_extensions_checksum(options: &ResolveProjectsOptions) -> String {
    let package_extensions = serde_json::to_value(
        options.transforms.package_extensions.as_ref().expect("package extensions are configured"),
    )
    .expect("package extensions serialize");
    hash_object_nullable_with_prefix(&package_extensions)
        .expect("configured package extensions have a checksum")
}

async fn assert_protocol_rejected(
    options: ResolveProjectsOptions,
    lockfile: Value,
    expected_message: &str,
) {
    let applied = matching_applied_policies(&options);
    let Err(PnprClientError::Protocol(message)) = resolve_mock_frames(
        options,
        vec![json!({ "type": "done", "lockfileFormat": "pnpm-lock/9.0", "lockfile": lockfile, "appliedPolicies": applied })],
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
    let body = frames
        .into_iter()
        .map(|frame| format!("{frame}\n"))
        .collect::<Vec<_>>()
        .concat();
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, supports_project_transforms).await;
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_body(body)
        .expect(usize::from(supports_project_transforms))
        .create_async()
        .await;

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
fn tarball_mismatch_keeps_its_own_variant() {
    let line = br#"{"type":"violations","violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#;
    let Frame::Violations { violations } = parse_frame(line).expect("frame parses") else {
        panic!("expected a violations frame");
    };
    let verify_err = build_verify_error(violations);
    assert!(matches!(verify_err, VerifyError::TarballUrlMismatch { .. }), "got {verify_err:?}");
}

#[test]
fn structural_violations_keep_their_code_and_hint() {
    for code in [
        "MISSING_TARBALL_INTEGRITY",
        "RESOLUTION_SHAPE_MISMATCH",
        "TARBALL_URL_MISMATCH",
        "TARBALL_REVISION_MISMATCH",
        "MISSING_NAMED_REGISTRY",
    ] {
        let line = format!(
            r#"{{"type":"violations","violations":[{{"name":"acme","version":"1.0.0","code":"{code}","reason":"broken"}},{{"name":"bravo","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"young"}}]}}"#,
        );
        let Frame::Violations { violations } = parse_frame(line.as_bytes()).expect("frame parses")
        else {
            panic!("expected a violations frame");
        };
        let verify_err = build_verify_error(violations);
        assert!(
            verify_err
                .to_string()
                .contains(&format!("[{code}]")),
            "got {verify_err}",
        );
        let help = miette::Diagnostic::help(&verify_err).expect("hint").to_string();
        assert!(!help.contains("relax the policy"), "{code}: {help}");
    }
}

#[test]
fn a_package_frame_parses_its_fetch_hint() {
    let line = br#"{"type":"package","id":"acme@1.0.0","name":"acme","version":"1.0.0","integrity":"sha512-abc","tarball":"https://r.test/acme/-/acme-1.0.0.tgz","unpackedSize":123456,"fileCount":42,"revision":3,"requiresBuild":false}"#;
    let Frame::Package {
        id,
        name,
        version,
        integrity,
        tarball,
        unpacked_size,
        file_count,
        revision,
        requires_build,
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
    assert_eq!(requires_build, Some(false));
}

#[test]
fn package_frames_reject_invalid_revisions() {
    for revision in ["-1", "9007199254740992"] {
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

#[tokio::test]
async fn pipeline_reports_refuse_credentials_on_non_loopback_http() {
    let client = PnprClient::new("http://example.invalid");
    let request = super::PublishPipelineRunRequest {
        org: "acme".to_string(),
        workspace: "demo".to_string(),
        run_id: "100-run".to_string(),
        summary: json!({}),
        events: Vec::new(),
    };
    let error = client
        .publish_pipeline_run(&request, Some("Bearer secret"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, PnprClientError::Protocol(_)),
        "insecure credentials must be rejected before any request: {error}",
    );
}

#[tokio::test]
async fn pipeline_report_redirects_are_not_followed() {
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, true).await;
    let redirect = server
        .mock("PUT", "/-/pnpr/v0/pipeline/runs")
        .with_status(307)
        .with_header("location", "/unexpected")
        .create_async()
        .await;
    let destination = server
        .mock("PUT", "/unexpected")
        .expect(0)
        .create_async()
        .await;
    let client = PnprClient::new(server.url());
    let request = super::PublishPipelineRunRequest {
        org: "acme".to_string(),
        workspace: "demo".to_string(),
        run_id: "100-run".to_string(),
        summary: json!({}),
        events: Vec::new(),
    };
    assert!(
        client
            .publish_pipeline_run(&request, Some("Bearer secret"))
            .await
            .is_err(),
        "report redirects must not receive credentials",
    );
    redirect.assert_async().await;
    destination.assert_async().await;
}

#[tokio::test]
async fn negotiates_v0_when_supported_by_handshake() {
    let mut server = mockito::Server::new_async().await;
    let handshake_mock = server
        .mock("GET", "/-/pnpr")
        .with_body(
            json!({
                "pnpr": {
                    "protocols": { "resolver": { "versions": [0], "features": ["fixLockfile", "projectTransforms"] } }
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let options = resolve_projects_options();
    let done = json!({
        "type": "done",
        "lockfileFormat": "pnpm-lock/9.0",
        "lockfile": matching_transform_lockfile(&options),
        "appliedPolicies": matching_applied_policies(&options)
    });

    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_body(format!("{done}\n"))
        .create_async()
        .await;

    let client = PnprClient::new(server.url());
    let version = client.negotiated_resolver_version().await.expect("negotiation succeeds");
    assert_eq!(version, 0);

    let outcome = client.resolve_projects(options).await.expect("resolve succeeds with v0");

    assert!(outcome.applied_policies.is_some());
    handshake_mock.assert_async().await;
    resolve_mock.assert_async().await;
}

#[tokio::test]
async fn structured_error_codes_are_preserved() {
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, true).await;
    let error_frame = json!({
        "type": "error",
        "message": "outdated lockfile",
        "code": "ERR_PNPM_OUTDATED_LOCKFILE"
    });

    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_body(format!("{error_frame}\n"))
        .create_async()
        .await;

    let err = PnprClient::new(server.url())
        .resolve_projects(resolve_projects_options())
        .await
        .unwrap_err();

    match err {
        PnprClientError::Server { message, code } => {
            assert_eq!(message, "outdated lockfile");
            assert_eq!(code.as_deref(), Some("ERR_PNPM_OUTDATED_LOCKFILE"));
        }
        other => panic!("expected PnprClientError::Server, got {other:?}"),
    }
    resolve_mock.assert_async().await;
}

#[tokio::test]
async fn unknown_frames_are_ignored() {
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, true).await;
    let warning_frame = json!({
        "type": "warning",
        "message": "new version available"
    });
    let options = resolve_projects_options();
    let done_frame = json!({
        "type": "done",
        "lockfileFormat": "pnpm-lock/9.0",
        "lockfile": matching_transform_lockfile(&options),
        "appliedPolicies": matching_applied_policies(&options)
    });

    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_body(format!("{warning_frame}\n{done_frame}\n"))
        .create_async()
        .await;

    let outcome = PnprClient::new(server.url())
        .resolve_projects(options)
        .await
        .expect("unknown frame should be ignored without error");

    assert_eq!(outcome.lockfile.lockfile_version.to_string(), "9.0");
    resolve_mock.assert_async().await;
}

#[tokio::test]
async fn fails_when_server_ignores_trust_policy() {
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, true).await;
    let mut opts = resolve_projects_options();
    opts.verification.trust_policy = TrustPolicy::NoDowngrade;

    let mut mismatched_policies = matching_applied_policies(&opts);
    mismatched_policies["trustPolicy"] = json!("off");
    let done = json!({
        "type": "done",
        "lockfileFormat": "pnpm-lock/9.0",
        "lockfile": matching_transform_lockfile(&opts),
        "appliedPolicies": mismatched_policies
    });

    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_body(format!("{done}\n"))
        .create_async()
        .await;

    let err = PnprClient::new(server.url()).resolve_projects(opts).await.unwrap_err();

    assert!(
        matches!(err, PnprClientError::Protocol(ref msg) if msg.contains("trustPolicy")),
        "expected error when server ignores trustPolicy, got: {err}",
    );
    resolve_mock.assert_async().await;
}

async fn mock_transform_handshake(
    server: &mut mockito::ServerGuard,
    supported: bool,
) -> mockito::Mock {
    server
        .mock("GET", "/-/pnpr")
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "pnpr": { "protocols": { "resolver": {
            "versions": [0], "features": if supported { vec!["projectTransforms"] } else { vec![] }
        } } } })
            .to_string(),
        )
        .expect_at_least(0)
        .create_async()
        .await
}

#[tokio::test]
async fn capability_handshake_ignores_unknown_protocols_and_features() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "pnpr": { "future": true, "protocols": {
            "resolver": { "versions": [0, 99], "features": ["futureFeature"], "future": 1 },
            "futureProtocol": { "versions": [42] }
        } } })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let client = PnprClient::new(server.url());
    client.handshake().await.unwrap();
    assert_eq!(client.negotiated_resolver_version().await.unwrap(), 0);
    handshake.assert_async().await;
}

fn matching_applied_policies(options: &ResolveProjectsOptions) -> Value {
    let mut applied = serde_json::to_value(&options.verification).unwrap();
    let object = applied.as_object_mut().unwrap();
    object.extend(
        serde_json::to_value(&options.resolution)
            .unwrap()
            .as_object()
            .unwrap()
            .clone(),
    );
    object.insert("frozenLockfile".into(), json!(options.reuse.frozen_lockfile));
    object.insert("trustLockfile".into(), json!(options.reuse.trust_lockfile));
    applied
}

#[tokio::test]
async fn resolve_rejects_missing_applied_policies() {
    for field in [
        "appliedPolicies",
        "minimumReleaseAge",
        "minimumReleaseAgeExclude",
        "minimumReleaseAgeIgnoreMissingTime",
        "trustPolicy",
        "trustPolicyExclude",
        "trustPolicyIgnoreAfter",
        "trustLockfile",
        "projectScopedOptionalPeers",
    ] {
        let options = resolve_projects_options();
        let mut done = json!({ "type": "done", "lockfileFormat": "pnpm-lock/9.0", "lockfile": matching_transform_lockfile(&options), "appliedPolicies": matching_applied_policies(&options) });
        if field == "appliedPolicies" {
            done.as_object_mut()
                .unwrap()
                .remove(field);
        } else {
            done["appliedPolicies"]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
        let result = resolve_mock_frames(options, vec![done], true, |_| {}).await;
        assert!(matches!(result, Err(PnprClientError::Protocol(_))), "{field}: {result:?}");
    }
}

#[tokio::test]
async fn verify_requires_matching_applied_policies() {
    for field in [
        "success",
        "appliedPolicies",
        "minimumReleaseAge",
        "minimumReleaseAgeExclude",
        "minimumReleaseAgeIgnoreMissingTime",
        "trustPolicy",
        "trustPolicyExclude",
        "trustPolicyIgnoreAfter",
        "trustLockfile",
    ] {
        let mut options = resolve_projects_options();
        options.reuse.lockfile =
            Some(serde_json::from_value(matching_transform_lockfile(&options)).unwrap());
        let mut applied = matching_applied_policies(&options);
        if field != "success" && field != "appliedPolicies" {
            applied[field] = mismatched_policy(field);
        }
        let mut done = json!({ "type": "done", "appliedPolicies": applied });
        if field == "appliedPolicies" {
            done.as_object_mut()
                .unwrap()
                .remove(field);
        }
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/-/pnpr/v0/verify-lockfile")
            .with_body(format!("{done}\n"))
            .create_async()
            .await;
        let result = PnprClient::new(server.url())
            .verify_lockfile(
                crate::VerifyLockfileOptions::from_resolve_projects_options(&options).unwrap(),
            )
            .await;
        if field == "success" {
            result.unwrap();
        } else {
            assert!(matches!(result, Err(PnprClientError::Protocol(_))), "{field}: {result:?}");
        }
        mock.assert_async().await;
    }
}

fn mismatched_policy(field: &str) -> Value {
    match field {
        "minimumReleaseAge" | "trustPolicyIgnoreAfter" => json!(0),
        "minimumReleaseAgeExclude" | "trustPolicyExclude" => json!([]),
        "minimumReleaseAgeIgnoreMissingTime" | "projectScopedOptionalPeers" => json!(true),
        "trustPolicy" => json!(null),
        "trustLockfile" | "addMissingPeerTypes" => json!(false),
        _ => panic!("no mismatch for {field}"),
    }
}

#[tokio::test]
async fn resolve_rejects_mismatched_policies() {
    for field in [
        "minimumReleaseAge",
        "minimumReleaseAgeExclude",
        "minimumReleaseAgeIgnoreMissingTime",
        "trustPolicy",
        "trustPolicyExclude",
        "trustPolicyIgnoreAfter",
        "trustLockfile",
        "addMissingPeerTypes",
        "projectScopedOptionalPeers",
    ] {
        let options = resolve_projects_options();
        let mut applied = matching_applied_policies(&options);
        applied[field] = mismatched_policy(field);
        let done = json!({ "type": "done", "lockfileFormat": "pnpm-lock/9.0", "lockfile": matching_transform_lockfile(&options), "appliedPolicies": applied });
        let result = resolve_mock_frames(options, vec![done], true, |_| {}).await;
        let Err(PnprClientError::Protocol(message)) = result else {
            panic!("{field}: expected a protocol error, got {result:?}");
        };
        let expected_message = match field {
            "addMissingPeerTypes" => "pnpr server ignored requested addMissingPeerTypes (expected Some(true), got false)".to_string(),
            "projectScopedOptionalPeers" => "pnpr server ignored requested projectScopedOptionalPeers (expected Some(false), got true)".to_string(),
            _ => format!("pnpr server ignored requested {field}"),
        };
        assert_eq!(message, expected_message, "{field}");
    }
}

#[tokio::test]
async fn http_errors_preserve_codes_and_display_the_reason() {
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, true).await;
    let response = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(403)
        .with_body(
            json!({
                "error": "Resolution denied",
                "reason": "Resolution denied",
                "code": "ERR_PNPR_FORBIDDEN"
            })
            .to_string(),
        )
        .create_async()
        .await;
    let error = PnprClient::new(server.url())
        .resolve_projects(resolve_projects_options())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("ERR_PNPR_FORBIDDEN"));
    let PnprClientError::Server { message, code } = error else {
        panic!("expected a server error");
    };
    assert_eq!(code.as_deref(), Some("ERR_PNPR_FORBIDDEN"));
    assert!(message.ends_with(": Resolution denied"), "{message}");
    response.assert_async().await;
}

#[tokio::test]
async fn handshake_errors_preserve_the_server_code() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/-/pnpr")
        .with_status(503)
        .with_body(
            json!({
                "error": "Service Unavailable",
                "reason": "Service Unavailable",
                "code": "ERR_PNPR_STORAGE_UNAVAILABLE"
            })
            .to_string(),
        )
        .create_async()
        .await;
    let error = PnprClient::new(server.url()).handshake().await.unwrap_err();
    let PnprClientError::Server { message, code } = error else {
        panic!("expected a server error");
    };
    assert_eq!(code.as_deref(), Some("ERR_PNPR_STORAGE_UNAVAILABLE"));
    assert!(message.ends_with(": Service Unavailable"), "{message}");
    response.assert_async().await;
}

#[tokio::test]
async fn a_stalled_handshake_error_body_has_a_deadline() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 1\r\nConnection: close\r\n\r\n")
            .await.unwrap();
        pending::<()>().await;
    });
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        PnprClient::new(format!("http://{address}")).handshake(),
    )
    .await
    .expect("the failed handshake body must time out")
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("/-/pnpr returned 503 Service Unavailable:"), "{message}");
    assert!(message.contains("timed out"), "{message}");
    server.abort();
}

#[tokio::test]
async fn resolve_rejects_a_done_frame_in_another_lockfile_format() {
    for lockfile_format in [None, Some("pnpm-lock/13.0"), Some("cargo-lock")] {
        let options = resolve_projects_options();
        let mut done = json!({
            "type": "done",
            "lockfile": matching_transform_lockfile(&options),
            "appliedPolicies": matching_applied_policies(&options),
        });
        if let Some(lockfile_format) = lockfile_format {
            done["lockfileFormat"] = json!(lockfile_format);
        }
        let result = resolve_mock_frames(options, vec![done], true, |_| {}).await;
        assert!(
            matches!(&result, Err(PnprClientError::Protocol(message)) if message.contains("lockfileFormat")),
            "{lockfile_format:?}: {result:?}",
        );
    }
}

#[tokio::test]
async fn resolve_sends_the_lockfile_formats_it_reads() {
    let options = resolve_projects_options();
    let done = json!({
        "type": "done",
        "lockfileFormat": "pnpm-lock/9.0",
        "lockfile": matching_transform_lockfile(&options),
        "appliedPolicies": matching_applied_policies(&options),
    });
    let mut server = mockito::Server::new_async().await;
    let _handshake = mock_transform_handshake(&mut server, true).await;
    let resolve_mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(json!({
            "acceptedLockfileFormats": ["pnpm-lock/9.0"],
        })))
        .with_body(format!("{done}\n"))
        .create_async()
        .await;

    let outcome =
        PnprClient::new(server.url()).resolve_projects(options).await.expect("resolve succeeds");
    assert!(outcome.applied_policies.is_some());
    resolve_mock.assert_async().await;
}

#[tokio::test]
async fn a_refused_setting_or_lockfile_format_is_an_unsupported_request() {
    for (code, unsupported) in [
        ("ERR_PNPR_UNSUPPORTED_REQUEST_FIELD", true),
        ("ERR_PNPR_INCOMPATIBLE_LOCKFILE_FORMAT", true),
        ("ERR_PNPR_FORBIDDEN", false),
        ("ERR_PNPR_BAD_REQUEST", false),
    ] {
        let mut server = mockito::Server::new_async().await;
        let _handshake = mock_transform_handshake(&mut server, true).await;
        let _resolve = server
            .mock("POST", "/-/pnpr/v0/resolve")
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_body(json!({ "error": "refused", "code": code }).to_string())
            .create_async()
            .await;

        let error = PnprClient::new(server.url())
            .resolve_projects(resolve_projects_options())
            .await
            .expect_err("the server refuses the request");

        assert!(
            matches!(&error, PnprClientError::Server { code: Some(got), .. } if got == code),
            "{error:?}",
        );
        assert_eq!(error.is_unsupported_request(), unsupported, "{code}");
    }
}

#[tokio::test]
async fn a_cargo_resolve_accepts_a_server_that_predates_lockfile_format() {
    for (done, accepted) in [
        (json!({ "type": "done", "lockfile": "version = 4\n" }), true),
        (
            json!({ "type": "done", "lockfileFormat": "cargo-lock", "lockfile": "version = 4\n" }),
            true,
        ),
        (json!({ "type": "done", "lockfileFormat": "pylock", "lockfile": "version = 4\n" }), false),
    ] {
        let mut server = mockito::Server::new_async().await;
        let _resolve = server
            .mock("POST", "/-/pnpr/v0/resolve")
            .with_body(format!("{done}\n"))
            .create_async()
            .await;

        let resolved = PnprClient::new(server.url())
            .resolve_cargo(super::CargoResolveOptions {
                metadata: "{}".to_string(),
                preferred_versions: BTreeMap::new(),
                registry: "https://index.crates.io/".to_string(),
                authorization: None,
            })
            .await;

        assert_eq!(resolved.is_ok(), accepted, "{done}: {resolved:?}");
    }
}

#[test]
fn a_sole_lockfile_format_may_be_omitted_but_not_replaced() {
    use super::lockfile_format::{PYLOCK, expect_sole_lockfile_format};

    assert!(expect_sole_lockfile_format(None, PYLOCK).is_ok());
    assert!(expect_sole_lockfile_format(Some(PYLOCK), PYLOCK).is_ok());
    assert!(expect_sole_lockfile_format(Some("cargo-lock"), PYLOCK).is_err());
}
