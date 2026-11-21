use super::{
    AllowAllVersions,
    Arc,
    Duration,
    HashMap,
    Identity,
    Mutex,
    ResolveRequest,
    RouteContext,
    cached_resolution,
    config,
    config_for_registry,
    lockfile,
    lockfile_tarball_url,
    lockfile_with_tarball,
    private_alias_footprint,
    public_registry_config,
    registry_config,
    reject_inline_url_auth,
    set_local_hosted_rules,
    tarball_router,
    tarball_router_with_registries,
    upstream_with_access,
    user,
};

#[test]
fn reject_inline_url_auth_scans_input_lockfile_tarballs() {
    // A lockfile tarball carrying inline `user:pass@host` credentials is
    // rejected before any fetch, so it can't reach the verify/frozen paths or
    // be echoed back.
    let dirty = ResolveRequest {
        lockfile: Some(lockfile_with_tarball(
            "https://user:pass@evil.example/acme/-/acme-1.0.0.tgz",
        )),
        ..ResolveRequest::default()
    };
    assert!(reject_inline_url_auth(&dirty).is_some());

    // A clean lockfile tarball is accepted.
    let clean = ResolveRequest {
        lockfile: Some(lockfile_with_tarball(
            "https://registry.example.test/acme/-/acme-1.0.0.tgz",
        )),
        ..ResolveRequest::default()
    };
    assert!(reject_inline_url_auth(&clean).is_none());
}

#[test]
fn private_cached_resolution_keeps_routed_tarball_urls() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let pnpm_config = config_for_registry("https://npm.corp.example/");
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
    let router = tarball_router(&registry, user("alice"));
    let routed = router.route_lockfile(&pnpm_config, &lockfile("1.0.0"));

    let allowed_picks = vec!["acme@1.0.0".to_string()];
    assert!(super::super::cache::store_resolution_with_allowed_picks(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp"),
        b"secret",
        &routed,
        allowed_picks.clone(),
    ));
    let cached = cached_resolution(
        &cache,
        Duration::from_mins(1),
        &key,
        &RouteContext::from_config(&registry),
        &user("alice"),
    )
    .expect("authorized caller reuses private cached lockfile");
    let tarball = lockfile_tarball_url(&cached.lockfile, "acme@1.0.0");

    assert!(tarball.contains("/npm/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
    assert_eq!(cached.allowed_picks, allowed_picks);
}

#[test]
fn a_package_frame_carries_unpacked_size_and_omits_it_when_unknown() {
    use pnpm_package_manager::{
        ResolutionObserver,
        ResolvedPackageHint,
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let registry = public_registry_config("https://r.test/");
    let observer = super::super::StreamObserver {
        tx,
        package_version_guard: Some(Arc::new(AllowAllVersions)),
        tarball_router: tarball_router(&registry, Identity::Anonymous),
        graph_limit: super::super::limits::GraphLimitTracker::new(16),
        cancel: super::super::limits::CancelToken::never(),
    };

    let hint = |unpacked_size, file_count, revision, requires_build| ResolvedPackageHint {
        integrity: "sha512-abc",
        tarball_url: "https://r.test/acme/-/acme-1.0.0.tgz",
        unpacked_size,
        file_count,
        revision,
        requires_build,
        from_registry: false,
        identity: pnpm_package_manager::ResolvedPackageIdentity {
            id: "acme@1.0.0",
            name: "acme",
            version: "1.0.0",
        },
    };
    observer.on_resolved(hint(Some(123_456), Some(42), Some(3), Some(false)));
    observer.on_resolved(hint(None, None, None, None));

    let sized: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("sized frame sent")).unwrap();
    assert_eq!(sized["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(sized["fileCount"], serde_json::json!(42));
    assert_eq!(sized["revision"], serde_json::json!(3));
    assert_eq!(sized["requiresBuild"], serde_json::json!(false));

    let unsized_frame: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("unsized frame sent")).unwrap();
    assert!(unsized_frame.get("unpackedSize").is_none());
    assert!(unsized_frame.get("fileCount").is_none());
    assert!(unsized_frame.get("revision").is_none());
    assert!(unsized_frame.get("requiresBuild").is_none());
    assert_eq!(unsized_frame["tarball"], serde_json::json!("https://r.test/acme/-/acme-1.0.0.tgz"));
}

#[test]
fn package_frames_route_private_alias_tarballs_to_gateway() {
    use pnpm_package_manager::ResolvedPackageHint;

    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let router = tarball_router(&registry, user("alice"));
    let frame = super::super::wire::package_frame(
        &router,
        &ResolvedPackageHint {
            integrity: "sha512-abc",
            tarball_url: "https://npm.corp.example/acme/-/acme-1.0.0.tgz",
            unpacked_size: None,
            file_count: None,
            revision: Some(3),
            requires_build: None,
            from_registry: false,
            identity: pnpm_package_manager::ResolvedPackageIdentity {
                id: "acme@1.0.0",
                name: "acme",
                version: "1.0.0",
            },
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    assert!(tarball.contains("/npm/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
    assert!(frame.get("revision").is_none());
}

#[test]
fn package_frame_routes_split_domain_registry_tarball_by_registry() {
    use pnpm_package_manager::ResolvedPackageHint;

    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    // The package resolves from the private corp registry, but its packument's
    // dist.tarball lives on a *different* host (a split-domain CDN).
    let registries =
        HashMap::from([("default".to_string(), "https://npm.corp.example/".to_string())]);
    let router = tarball_router_with_registries(&registry, user("alice"), registries);
    let frame = super::super::wire::package_frame(
        &router,
        &ResolvedPackageHint {
            integrity: "sha512-abc",
            tarball_url: "https://cdn.split-domain.example/acme-1.0.0.tgz",
            unpacked_size: None,
            file_count: None,
            revision: None,
            requires_build: None,
            from_registry: true,
            identity: pnpm_package_manager::ResolvedPackageIdentity {
                id: "acme@1.0.0",
                name: "acme",
                version: "1.0.0",
            },
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    // Routed by the corp registry, not the CDN host, so the raw upstream CDN
    // URL is never emitted to the client.
    assert!(tarball.contains("/npm/~corp/acme/-/acme-1.0.0.tgz"), "got {tarball}");
    assert!(!tarball.contains("split-domain.example"), "raw CDN URL leaked: {tarball}");
}

#[test]
fn package_frame_strips_signed_token_from_public_registry_tarball() {
    use pnpm_package_manager::ResolvedPackageHint;

    let registry = registry_config();
    let registries =
        HashMap::from([("default".to_string(), "https://registry.npmjs.org/".to_string())]);
    let router = tarball_router_with_registries(&registry, user("alice"), registries);
    let frame = super::super::wire::package_frame(
        &router,
        &ResolvedPackageHint {
            integrity: "sha512-abc",
            // A public registry that fronts a presigned CDN URL with a token.
            tarball_url: "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz?token=secret",
            unpacked_size: None,
            file_count: None,
            revision: None,
            requires_build: None,
            from_registry: true,
            identity: pnpm_package_manager::ResolvedPackageIdentity {
                id: "acme@1.0.0",
                name: "acme",
                version: "1.0.0",
            },
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    // The upstream token is never emitted to the client.
    assert_eq!(tarball, "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz", "got {tarball}");
}

#[test]
fn frozen_package_frames_announce_lockfile_tarballs_with_sizes() {
    use pnpm_lockfile::Lockfile;
    use pnpm_resolving_npm_resolver::{
        DistStats,
        observed_dist_stats_key,
        observed_dist_stats_sink,
    };

    let lockfile: Lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": { "dependencies": { "acme": { "specifier": "^1.0.0", "version": "1.0.0" } } }
        },
        "packages": {
            "acme@1.0.0": {
                "resolution": { "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
            },
            "linked-dir@1.0.0": {
                "resolution": { "type": "directory", "directory": "../linked-dir" }
            }
        }
    }))
    .expect("lockfile parses");

    let stats = observed_dist_stats_sink();
    stats.insert(
        observed_dist_stats_key(
            "https://registry.example.test/",
            "acme".to_owned(),
            "1.0.0".to_owned(),
        ),
        DistStats {
            unpacked_size: Some(123_456),
            file_count: Some(42),
            requires_build: Some(true),
        },
    );

    let registry = public_registry_config("https://registry.example.test/");
    let frames = super::super::wire::frozen_package_frames_until(
        &config(),
        &tarball_router(&registry, Identity::Anonymous),
        &lockfile,
        &stats,
        &|| false,
    )
    .unwrap();
    assert_eq!(frames.len(), 1);

    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    assert_eq!(frame["type"], serde_json::json!("package"));
    assert_eq!(frame["id"], serde_json::json!("acme@1.0.0"));
    assert_eq!(
        frame["tarball"],
        serde_json::json!("https://registry.example.test/acme/-/acme-1.0.0.tgz"),
    );
    assert_eq!(frame["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(frame["fileCount"], serde_json::json!(42));
    assert_eq!(frame["requiresBuild"], serde_json::json!(true));
}

#[test]
fn frozen_package_frames_route_private_alias_tarballs_to_gateway() {
    use pnpm_resolving_npm_resolver::observed_dist_stats_sink;

    let pnpm_config = config_for_registry("https://npm.corp.example/");
    let lockfile = lockfile("1.0.0");
    let stats = observed_dist_stats_sink();
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );

    let frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &tarball_router(&registry, user("alice")),
        &lockfile,
        &stats,
        &|| false,
    )
    .unwrap();

    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    let tarball = frame["tarball"].as_str().expect("tarball URL");
    assert!(tarball.contains("/npm/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
}

#[test]
fn osv_checkable_tarball_does_not_trust_git_hosted_flag_or_strict_url_parsing() {
    use super::super::wire::osv::is_osv_checkable_resolution;
    use pnpm_lockfile::{
        LockfileResolution,
        TarballResolution,
    };

    let tarball = |url: &str, git_hosted: Option<bool>| {
        LockfileResolution::Tarball(TarballResolution {
            tarball: url.to_string(),
            integrity: None,
            revision: None,
            git_hosted,
            path: None,
        })
    };

    // `gitHosted: true` must not let a normal https registry tarball opt out.
    assert!(is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
        Some(true),
    )));
    // A URL that strict parsing would reject is still scanned when it is HTTP or HTTPS.
    assert!(is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo 1.0.0.tgz",
        None,
    )));
    // Mutable git-host archive refs are still checked.
    assert!(is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/abc123",
        Some(false),
    )));
    // Genuinely git-hosted-by-URL tarballs are skipped regardless of the flag.
    assert!(!is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567",
        Some(false),
    )));
    // Non-http schemes are skipped.
    assert!(!is_osv_checkable_resolution(&tarball("file:../foo.tgz", None)));
}

#[test]
fn tarball_url_version_extracts_conventional_names_only() {
    use super::super::wire::osv::tarball_url_version;

    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.tgz", "foo"), Some("1.2.3"));
    // Scoped packages name the tarball file with the unscoped name.
    assert_eq!(tarball_url_version("https://r/@s/foo/-/foo-1.2.3.tgz", "@s/foo"), Some("1.2.3"));
    // Query/fragment are stripped; prerelease/build keep working.
    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.tgz?x=1", "foo"), Some("1.2.3"));
    assert_eq!(
        tarball_url_version("https://r/foo/-/foo-1.2.3-beta.1.tgz", "foo"),
        Some("1.2.3-beta.1"),
    );
    // Suffix matching is case-insensitive and covers `.tar.gz`, so a
    // tampered lockfile can't dodge the cross-check with a variant.
    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.TGZ", "foo"), Some("1.2.3"));
    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.tar.gz", "foo"), Some("1.2.3"));
    // Non-conventional naming yields None (fall back, don't misjudge).
    assert_eq!(tarball_url_version("https://r/weird.tgz", "foo"), None);
    assert_eq!(tarball_url_version("https://r/foo/-/foo.tgz", "foo"), None);
}

#[test]
fn npm_success_frames_echo_every_policy() {
    use super::super::{
        protocol::AppliedPolicies,
        wire::{
            try_done_frame_until,
            verify_done_frame,
        },
    };
    use serde_json::{
        Value,
        json,
    };

    let request: ResolveRequest = serde_json::from_value(json!({
        "dedupePeers": true,
        "minimumReleaseAge": 60,
        "minimumReleaseAgeExclude": ["example-*"],
        "minimumReleaseAgeIgnoreMissingTime": false,
        "minimumReleaseAgeStrict": false,
        "trustPolicy": "no-downgrade",
        "trustPolicyExclude": ["legacy-*"],
        "trustPolicyIgnoreAfter": 1440,
        "trustLockfile": false,
        "projectScopedOptionalPeers": true,
    }))
    .unwrap();
    let applied = AppliedPolicies::from_request(&request);
    let expected = json!({
        "autoInstallPeers": true,
        "dedupePeers": true,
        "excludeLinksFromLockfile": false,
        "addMissingPeerTypes": false,
        "projectScopedOptionalPeers": true,
        "frozenLockfile": false,
        "resolutionMode": "highest",
        "minimumReleaseAge": 60,
        "minimumReleaseAgeExclude": ["example-*"],
        "minimumReleaseAgeIgnoreMissingTime": false,
        "minimumReleaseAgeStrict": false,
        "trustPolicy": "no-downgrade",
        "trustPolicyExclude": ["legacy-*"],
        "trustPolicyIgnoreAfter": 1440,
        "trustLockfile": false,
    });
    let lockfile = lockfile("1.0.0");
    let resolve_bytes = try_done_frame_until(&lockfile, &applied, &|| false).unwrap().unwrap();
    let resolve: Value = serde_json::from_slice(&resolve_bytes).unwrap();
    assert_eq!(resolve["appliedPolicies"], expected);
    assert_eq!(resolve["lockfileFormat"], "pnpm-lock/9.0");
    let verify: Value = serde_json::from_slice(&verify_done_frame(&request)).unwrap();
    assert_eq!(verify["appliedPolicies"], expected);

    let defaults =
        serde_json::to_value(AppliedPolicies::from_request(&ResolveRequest::default())).unwrap();
    assert_eq!(defaults["minimumReleaseAge"], Value::Null);
    assert_eq!(defaults["minimumReleaseAgeExclude"], json!([]));
    assert_eq!(defaults["minimumReleaseAgeIgnoreMissingTime"], json!(true));
    assert_eq!(defaults["minimumReleaseAgeStrict"], json!(false));
    assert_eq!(defaults["trustPolicy"], Value::Null);
    assert_eq!(defaults["trustPolicyExclude"], json!([]));
    assert_eq!(defaults["trustPolicyIgnoreAfter"], Value::Null);
}

#[test]
fn streaming_errors_share_the_http_error_fields() {
    for (code, expected) in [
        (None, "ERR_PNPR_RESOLUTION_FAILED"),
        (Some("ERR_PNPM_OUTDATED_LOCKFILE"), "ERR_PNPR_OUTDATED_LOCKFILE"),
    ] {
        let bytes = super::super::wire::error_frame_with_code("Resolution failed", code);
        assert_eq!(bytes.last(), Some(&b'\n'));
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "type": "error", "message": "Resolution failed",
                "error": "Resolution failed", "reason": "Resolution failed", "code": expected,
            }),
        );
    }
}

#[test]
fn a_cargo_done_frame_names_its_lockfile_format() {
    use super::cargo_done_frame;

    let cargo: serde_json::Value =
        serde_json::from_slice(&cargo_done_frame("version = 4\n")).unwrap();
    assert_eq!(cargo["lockfileFormat"], "cargo-lock");
    assert_eq!(cargo["lockfile"], "version = 4\n");
}

fn split_domain_lockfile(key: &str, dep_version: &str, tarball: &str) -> pnpm_lockfile::Lockfile {
    serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": { "dependencies": { "@corp/split": { "specifier": "1.0.0", "version": dep_version } } }
        },
        "packages": {
            (key): {
                "resolution": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": tarball,
                }
            }
        }
    }))
    .expect("split-domain lockfile parses")
}

#[test]
fn lockfile_and_frozen_frames_route_split_domain_private_upstream_tarballs_to_alias() {
    use pnpm_resolving_npm_resolver::observed_dist_stats_sink;

    let mut pnpm_config = config_for_registry("https://registry.npmjs.org/");
    pnpm_config.registries_by_scope.insert(
        "@corp".to_string(),
        "https://npm.corp.example/".to_string(),
    );
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
    let router = tarball_router(&registry, user("alice"));
    let cdn_url = "https://cdn.split-domain.example/@corp/split/split-1.0.0.tgz?X-Amz-Signature=EXAMPLE_PRESIGNED_TOKEN";
    let raw = split_domain_lockfile("@corp/split@1.0.0", "1.0.0", cdn_url);

    let routed = router.route_lockfile(&pnpm_config, &raw);
    let lockfile_url = lockfile_tarball_url(&routed, "@corp/split@1.0.0");
    assert_eq!(lockfile_url, "http://127.0.0.1:7677/npm/~corp/@corp/split/-/split-1.0.0.tgz");

    let frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router,
        &raw,
        &observed_dist_stats_sink(),
        &|| false,
    )
    .unwrap();
    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    assert_eq!(frame["tarball"], lockfile_url);
}

#[test]
fn lockfile_and_frozen_frames_route_named_registry_split_domain_tarballs_to_alias() {
    use pnpm_resolving_npm_resolver::observed_dist_stats_sink;

    let mut pnpm_config = config_for_registry("https://registry.npmjs.org/");
    pnpm_config.registries_by_prefix.insert(
        "corp".to_string(),
        "https://npm.corp.example/".to_string(),
    );
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
    let router = tarball_router(&registry, user("alice"))
        .with_registries_by_prefix(pnpm_config.registries_by_prefix.clone());
    let cdn_url = "https://cdn.split-domain.example/@corp/split/split-1.0.0.tgz?X-Amz-Signature=EXAMPLE_PRESIGNED_TOKEN";
    let raw = split_domain_lockfile("@corp/split@corp:1.0.0", "corp:1.0.0", cdn_url);

    let routed = router.route_lockfile(&pnpm_config, &raw);
    let lockfile_url = lockfile_tarball_url(&routed, "@corp/split@corp:1.0.0");
    assert_eq!(lockfile_url, "http://127.0.0.1:7677/npm/~corp/@corp/split/-/split-1.0.0.tgz");

    let frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router,
        &raw,
        &observed_dist_stats_sink(),
        &|| false,
    )
    .unwrap();
    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    assert_eq!(frame["tarball"], lockfile_url);
    assert_eq!(frame["version"], "1.0.0");
}

#[test]
fn public_registry_lockfile_strips_query_tokens_and_never_routes_spoofed_tarball_to_private_alias()
{
    use pnpm_resolving_npm_resolver::observed_dist_stats_sink;

    let pnpm_config = config_for_registry("https://registry.npmjs.org/");
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
    let router = tarball_router(&registry, user("alice"));

    let signed_public = lockfile_with_tarball(
        "https://cdn.public.example/acme/-/acme-1.0.0.tgz?X-Amz-Signature=EXAMPLE_PRESIGNED_TOKEN",
    );
    let routed_public = router.route_lockfile(&pnpm_config, &signed_public);
    assert_eq!(
        lockfile_tarball_url(&routed_public, "acme@1.0.0"),
        "https://cdn.public.example/acme/-/acme-1.0.0.tgz",
    );
    let frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router,
        &signed_public,
        &observed_dist_stats_sink(),
        &|| false,
    )
    .unwrap();
    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    assert_eq!(frame["tarball"], "https://cdn.public.example/acme/-/acme-1.0.0.tgz");

    // A public package whose untrusted `dist.tarball` points at `corp`'s origin
    // must still be classified as public (by its registry), never minted as `/~corp/`
    // in either the lockfile or the frozen `package` frame.
    let spoofed = lockfile_with_tarball("https://npm.corp.example/acme/-/acme-1.0.0.tgz");
    let routed_spoofed = router.route_lockfile(&pnpm_config, &spoofed);
    assert_eq!(
        lockfile_tarball_url(&routed_spoofed, "acme@1.0.0"),
        "https://npm.corp.example/acme/-/acme-1.0.0.tgz",
    );
    let spoofed_frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router,
        &spoofed,
        &observed_dist_stats_sink(),
        &|| false,
    )
    .unwrap();
    let spoofed_frame: serde_json::Value = serde_json::from_slice(&spoofed_frames[0]).unwrap();
    assert_eq!(spoofed_frame["tarball"], "https://npm.corp.example/acme/-/acme-1.0.0.tgz");
}

#[test]
fn published_tarball_mapper_accepts_public_query_urls_and_binds_private_entries_individually() {
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
    let router = tarball_router(&registry, user("alice"));
    let mapper = router.published_tarball_mapper();

    let public_published = "https://cdn.public.example/acme/-/acme-1.0.0.tgz?download=1";
    let public_mapped =
        mapper("https://registry.npmjs.org/", "acme", "1.0.0", public_published, public_published);
    assert!(public_mapped.accept_published_url);
    assert_eq!(
        public_mapped.accepted_routed_url.as_deref(),
        Some("https://cdn.public.example/acme/-/acme-1.0.0.tgz"),
    );

    let private_published =
        "https://cdn.corp.example/@corp/split/split-1.0.0.tgz?X-Amz-Signature=EXAMPLE_TOKEN";
    let routed = "http://127.0.0.1:7677/npm/~corp/@corp/split/-/split-1.0.0.tgz";
    let routed_mapped =
        mapper("https://npm.corp.example/", "@corp/split", "1.0.0", routed, private_published);
    assert!(!routed_mapped.accept_published_url);
    assert_eq!(routed_mapped.accepted_routed_url.as_deref(), Some(routed));

    let unrouted_mapped = mapper(
        "https://npm.corp.example/",
        "@corp/split",
        "1.0.0",
        "https://npm.corp.example/@corp/split/-/split-1.0.0.tgz",
        private_published,
    );
    assert!(!unrouted_mapped.accept_published_url);
    assert_eq!(unrouted_mapped.accepted_routed_url, None);

    // A lockfile without `/~` endpoint URLs still hashes differently for a
    // caller whose registry is `Proxied` (`alice`) vs `Public` (`bob`).
    let pnpm_config = config_for_registry("https://npm.corp.example/");
    let raw = lockfile_with_tarball("https://npm.corp.example/acme/-/acme-1.0.0.tgz");
    let outsider_router = tarball_router(&registry, user("bob"));
    assert_ne!(
        router.verification_verdict_hash(&pnpm_config, &raw),
        outsider_router.verification_verdict_hash(&pnpm_config, &raw),
    );
    assert!(
        router
            .verification_verdict_hash_until(&pnpm_config, &raw, &|| true)
            .is_none(),
    );

    // Hosted (default and addressed `/~local`) and proxied (`/~corp`) revision
    // URLs (`{pkg}@{version}+r{N}`) preserve their revision route instead of
    // turning into `{pkg}/-/{pkg}@{version}+r{N}`.
    let mut hosted_cfg = registry_config();
    set_local_hosted_rules(&mut hosted_cfg, "@private/*", "alice");
    let hosted_mapper = tarball_router(&hosted_cfg, user("alice")).published_tarball_mapper();
    let hosted_rev = "http://127.0.0.1:7677/npm/@private/rev@1.0.0+r3";
    let hosted_mapped = hosted_mapper(
        "http://127.0.0.1:7677/npm/",
        "@private/rev",
        "1.0.0",
        hosted_rev,
        hosted_rev,
    );
    assert_eq!(hosted_mapped.accepted_routed_url.as_deref(), Some(hosted_rev));

    let addressed_rev = "http://127.0.0.1:7677/npm/~local/@private/rev@1.0.0+r3";
    let addressed_mapped = hosted_mapper(
        "http://127.0.0.1:7677/npm/~local/",
        "@private/rev",
        "1.0.0",
        addressed_rev,
        addressed_rev,
    );
    assert_eq!(addressed_mapped.accepted_routed_url.as_deref(), Some(addressed_rev));

    let upstream_rev = "https://npm.corp.example/@corp/rev@1.0.0+r3";
    let proxied_rev = "http://127.0.0.1:7677/npm/~corp/@corp/rev@1.0.0+r3";
    let proxied_mapped =
        mapper("https://npm.corp.example/", "@corp/rev", "1.0.0", proxied_rev, upstream_rev);
    assert_eq!(proxied_mapped.accepted_routed_url.as_deref(), Some(proxied_rev));
}

#[test]
fn frozen_frames_and_lockfile_routing_isolate_multi_registry_stats_and_routed_endpoints() {
    use pnpm_lockfile::Lockfile;
    use pnpm_resolving_npm_resolver::{
        DistStats,
        observed_dist_stats_key,
        observed_dist_stats_sink,
    };

    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "east".to_string(),
        upstream_with_access("https://east.corp.example/", "alice"),
    );
    registry.routing.upstreams.insert(
        "west".to_string(),
        upstream_with_access("https://west.corp.example/", "alice"),
    );
    let mut pnpm_config = config_for_registry("https://west.corp.example/");
    pnpm_config.registries_by_prefix.insert(
        "east".to_string(),
        "https://east.corp.example/".to_string(),
    );
    pnpm_config.registries_by_prefix.insert(
        "west".to_string(),
        "https://west.corp.example/".to_string(),
    );
    let router = tarball_router(&registry, user("alice"))
        .with_registries_by_prefix(pnpm_config.registries_by_prefix.clone());

    // 1. Same name@version across `east:` and `west:` keeps distinct dist stats.
    let multi_lockfile: Lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": { ".": {} },
        "packages": {
            "acme@east:1.0.0": {
                "resolution": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": "http://127.0.0.1:7677/npm/~east/acme/-/acme-1.0.0.tgz"
                }
            },
            "acme@west:1.0.0": {
                "resolution": {
                    "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                    "tarball": "http://127.0.0.1:7677/npm/~west/acme/-/acme-1.0.0.tgz"
                }
            }
        }
    }))
    .unwrap();
    let stats = observed_dist_stats_sink();
    stats.insert(
        observed_dist_stats_key(
            "https://east.corp.example/",
            "acme".to_owned(),
            "1.0.0".to_owned(),
        ),
        DistStats { unpacked_size: Some(1_111), file_count: Some(10), requires_build: Some(true) },
    );
    stats.insert(
        observed_dist_stats_key(
            "https://west.corp.example/",
            "acme".to_owned(),
            "1.0.0".to_owned(),
        ),
        DistStats { unpacked_size: Some(2_222), file_count: Some(20), requires_build: Some(false) },
    );
    let frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router,
        &multi_lockfile,
        &stats,
        &|| false,
    )
    .unwrap();
    assert_eq!(frames.len(), 2);
    let parsed: Vec<serde_json::Value> = frames
        .iter()
        .map(|line| serde_json::from_slice(line))
        .map(Result::unwrap)
        .collect();
    let east_frame = parsed
        .iter()
        .find(|frame| frame["id"] == "acme@east:1.0.0")
        .unwrap();
    let west_frame = parsed
        .iter()
        .find(|frame| frame["id"] == "acme@west:1.0.0")
        .unwrap();
    assert_eq!(east_frame["unpackedSize"], 1_111);
    assert_eq!(east_frame["requiresBuild"], true);
    assert_eq!(west_frame["unpackedSize"], 2_222);
    assert_eq!(west_frame["requiresBuild"], false);

    let unprefixed_routed =
        lockfile_with_tarball("http://127.0.0.1:7677/npm/~east/acme/-/acme-1.0.0.tgz");
    let routed = router.route_lockfile(&pnpm_config, &unprefixed_routed);
    assert_eq!(
        lockfile_tarball_url(&routed, "acme@1.0.0"),
        "http://127.0.0.1:7677/npm/~east/acme/-/acme-1.0.0.tgz",
    );
    let unprefixed_frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router,
        &unprefixed_routed,
        &stats,
        &|| false,
    )
    .unwrap();
    let unprefixed_frame: serde_json::Value =
        serde_json::from_slice(&unprefixed_frames[0]).unwrap();
    assert_eq!(
        unprefixed_frame["tarball"],
        "http://127.0.0.1:7677/npm/~east/acme/-/acme-1.0.0.tgz",
    );
    assert_eq!(unprefixed_frame["unpackedSize"], 1_111);
    assert_eq!(unprefixed_frame["requiresBuild"], true);

    pnpm_config.registries_by_prefix.insert(
        "pub".to_string(),
        "https://pub.east.example/".to_string(),
    );
    let router_pub = router.with_registries_by_prefix(pnpm_config.registries_by_prefix.clone());
    let signed_under_pub =
        lockfile_with_tarball("https://pub.east.example/acme/-/acme-1.0.0.tgz?X-Amz-Signature=SIG");
    assert_eq!(
        lockfile_tarball_url(
            &router_pub.route_lockfile(&pnpm_config, &signed_under_pub),
            "acme@1.0.0",
        ),
        "http://127.0.0.1:7677/npm/~west/acme/-/acme-1.0.0.tgz",
    );
    let raw_pub = lockfile_with_tarball("https://pub.east.example/acme/-/acme-1.0.0.tgz");
    let routed_pub = router_pub.route_input_lockfile(&pnpm_config, &raw_pub);
    assert_eq!(
        lockfile_tarball_url(&routed_pub, "acme@1.0.0"),
        "https://pub.east.example/acme/-/acme-1.0.0.tgz",
    );
    let reused_pub = router_pub
        .route_lockfile_until(
            &pnpm_config,
            &raw_pub,
            super::super::wire::LockfileOrigin::Resolved { input: Some(&raw_pub) },
            &|| false,
        )
        .unwrap();
    assert_eq!(
        lockfile_tarball_url(&reused_pub, "acme@1.0.0"),
        "https://pub.east.example/acme/-/acme-1.0.0.tgz",
    );
    let pub_frames = super::super::wire::frozen_package_frames_until(
        &pnpm_config,
        &router_pub,
        &raw_pub,
        &stats,
        &|| false,
    )
    .unwrap();
    let pub_frame: serde_json::Value = serde_json::from_slice(&pub_frames[0]).unwrap();
    assert_eq!(pub_frame["tarball"], "https://pub.east.example/acme/-/acme-1.0.0.tgz");
}
