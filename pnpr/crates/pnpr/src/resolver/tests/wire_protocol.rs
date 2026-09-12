use super::{
    AllowAllVersions, Arc, Duration, HashMap, Identity, Mutex, ResolveRequest, RouteContext,
    cached_resolution, config, config_for_registry, lockfile, lockfile_tarball_url,
    lockfile_with_tarball, private_alias_footprint, public_registry_config, registry_config,
    reject_inline_url_auth, store_resolution, tarball_router, tarball_router_with_registries,
    upstream_with_access, user,
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
    registry
        .upstreams
        .insert("corp".to_string(), upstream_with_access("https://npm.corp.example/", "alice"));
    let router = tarball_router(&registry, user("alice"));
    let routed = router.route_lockfile(&pnpm_config, &lockfile("1.0.0"));

    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp"),
        b"secret",
        &routed,
    ));
    let cached = cached_resolution(
        &cache,
        Duration::from_mins(1),
        &key,
        &RouteContext::from_config(&registry),
        &user("alice"),
    )
    .expect("authorized caller reuses private cached lockfile");
    let tarball = lockfile_tarball_url(&cached, "acme@1.0.0");

    assert!(tarball.contains("/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
}

#[test]
fn a_package_frame_carries_unpacked_size_and_omits_it_when_unknown() {
    use pnpm_package_manager::{ResolutionObserver, ResolvedPackageHint};

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let registry = public_registry_config("https://r.test/");
    let observer = super::super::StreamObserver {
        tx,
        package_version_guard: Some(Arc::new(AllowAllVersions)),
        tarball_router: tarball_router(&registry, Identity::Anonymous),
    };

    let hint = |unpacked_size, file_count, revision| ResolvedPackageHint {
        id: "acme@1.0.0",
        name: "acme",
        version: "1.0.0",
        integrity: "sha512-abc",
        tarball_url: "https://r.test/acme/-/acme-1.0.0.tgz",
        unpacked_size,
        file_count,
        revision,
        from_registry: false,
    };
    observer.on_resolved(hint(Some(123_456), Some(42), Some(3)));
    observer.on_resolved(hint(None, None, None));

    let sized: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("sized frame sent")).unwrap();
    assert_eq!(sized["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(sized["fileCount"], serde_json::json!(42));
    assert_eq!(sized["revision"], serde_json::json!(3));

    let unsized_frame: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("unsized frame sent")).unwrap();
    assert!(unsized_frame.get("unpackedSize").is_none());
    assert!(unsized_frame.get("fileCount").is_none());
    assert!(unsized_frame.get("revision").is_none());
    assert_eq!(unsized_frame["tarball"], serde_json::json!("https://r.test/acme/-/acme-1.0.0.tgz"));
}

#[test]
fn package_frames_route_private_alias_tarballs_to_gateway() {
    use pnpm_package_manager::ResolvedPackageHint;

    let mut registry = registry_config();
    registry.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let router = tarball_router(&registry, user("alice"));
    let frame = super::super::wire::package_frame(
        &router,
        &ResolvedPackageHint {
            id: "acme@1.0.0",
            name: "acme",
            version: "1.0.0",
            integrity: "sha512-abc",
            tarball_url: "https://npm.corp.example/acme/-/acme-1.0.0.tgz",
            unpacked_size: None,
            file_count: None,
            revision: Some(3),
            from_registry: false,
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    assert!(tarball.contains("/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
    assert!(frame.get("revision").is_none());
}

#[test]
fn package_frame_routes_split_domain_registry_tarball_by_registry() {
    use pnpm_package_manager::ResolvedPackageHint;

    let mut registry = registry_config();
    registry.upstreams.insert(
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
            id: "acme@1.0.0",
            name: "acme",
            version: "1.0.0",
            integrity: "sha512-abc",
            tarball_url: "https://cdn.split-domain.example/acme-1.0.0.tgz",
            unpacked_size: None,
            file_count: None,
            revision: None,
            from_registry: true,
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    // Routed by the corp registry, not the CDN host — so the raw upstream CDN
    // URL is never emitted to the client.
    assert!(tarball.contains("/~corp/acme/-/acme-1.0.0.tgz"), "got {tarball}");
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
            id: "acme@1.0.0",
            name: "acme",
            version: "1.0.0",
            integrity: "sha512-abc",
            // A public registry that fronts a presigned CDN URL with a token.
            tarball_url: "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz?token=secret",
            unpacked_size: None,
            file_count: None,
            revision: None,
            from_registry: true,
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    // The upstream token is never emitted to the client.
    assert_eq!(tarball, "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz", "got {tarball}");
}

#[test]
fn frozen_package_frames_announce_lockfile_tarballs_with_sizes() {
    use pnpm_lockfile::Lockfile;
    use pnpm_resolving_npm_resolver::{DistStats, observed_dist_stats_sink};

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
        ("acme".to_string(), "1.0.0".to_string()),
        DistStats { unpacked_size: Some(123_456), file_count: Some(42) },
    );

    let registry = public_registry_config("https://registry.example.test/");
    let frames = super::super::frozen_package_frames(
        &config(),
        &tarball_router(&registry, Identity::Anonymous),
        &lockfile,
        &stats,
    );
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
}

#[test]
fn frozen_package_frames_route_private_alias_tarballs_to_gateway() {
    use pnpm_resolving_npm_resolver::observed_dist_stats_sink;

    let pnpm_config = config_for_registry("https://npm.corp.example/");
    let lockfile = lockfile("1.0.0");
    let stats = observed_dist_stats_sink();
    let mut registry = registry_config();
    registry.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );

    let frames = super::super::frozen_package_frames(
        &pnpm_config,
        &tarball_router(&registry, user("alice")),
        &lockfile,
        &stats,
    );

    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    let tarball = frame["tarball"].as_str().expect("tarball URL");
    assert!(tarball.contains("/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
}

#[test]
fn osv_checkable_tarball_does_not_trust_git_hosted_flag_or_strict_url_parsing() {
    use pnpm_lockfile::{LockfileResolution, TarballResolution};

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
    assert!(super::super::wire::is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
        Some(true),
    )));
    // A URL that strict parsing would reject is still scanned when it is http(s).
    assert!(super::super::wire::is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo 1.0.0.tgz",
        None,
    )));
    // Mutable git-host archive refs are still checked.
    assert!(super::super::wire::is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/abc123",
        Some(false),
    )));
    // Genuinely git-hosted-by-URL tarballs are skipped regardless of the flag.
    assert!(!super::super::wire::is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567",
        Some(false),
    )));
    // Non-http schemes are skipped.
    assert!(!super::super::wire::is_osv_checkable_resolution(&tarball("file:../foo.tgz", None)));
}

#[test]
fn tarball_url_version_extracts_conventional_names_only() {
    use super::super::wire::tarball_url_version;

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
