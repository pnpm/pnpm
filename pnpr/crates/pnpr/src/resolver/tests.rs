use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::Lockfile;
use pacquet_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
};
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use super::{
    MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY, cached_resolution,
    protocol::{ResolveRequest, ResolveRequestProject},
    resolution_cache_key, store_resolution,
};
use crate::{
    config::{Config as RegistryConfig, PublicRoute, UplinkConfig},
    policy::{AccessList, Identity, PackagePolicies, PackagePolicy},
    route::{Footprint, PrivateAccessDescriptor, RouteContext},
};

fn config_for_registry(registry: &str) -> PacquetConfig {
    let mut config = PacquetConfig::new();
    config.registry = registry.to_string();
    config
}

fn config() -> PacquetConfig {
    config_for_registry("https://registry.example.test/")
}

fn deps(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(name, spec)| ((*name).to_string(), (*spec).to_string())).collect()
}

fn registry_config() -> RegistryConfig {
    RegistryConfig::proxy(
        "127.0.0.1:7677".parse::<SocketAddr>().unwrap(),
        PathBuf::from("/tmp/pnpr-resolver-cache-test"),
    )
}

fn public_registry_config(registry: &str) -> RegistryConfig {
    let mut config = registry_config();
    config
        .route_policy
        .public
        .push(PublicRoute { registry: Some(registry.to_string()), package: None });
    config
}

fn tarball_router(config: &RegistryConfig, identity: Identity) -> super::TarballRouter {
    tarball_router_with_registries(config, identity, HashMap::new())
}

fn tarball_router_with_registries(
    config: &RegistryConfig,
    identity: Identity,
    registries: HashMap<String, String>,
) -> super::TarballRouter {
    super::TarballRouter::new(
        Arc::new(RouteContext::from_config(config)),
        identity,
        config.public_url.clone(),
        registries,
    )
}

fn user(name: &str) -> Identity {
    Identity::user(name)
}

fn uplink_with_access(registry: &str, access: &str, generation: u64) -> UplinkConfig {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_static("Bearer alias-secret"),
    );
    let mut uplink = UplinkConfig::with_defaults(registry.to_string(), headers);
    uplink.access = Some(AccessList::parse(access));
    uplink.generation = generation;
    uplink
}

fn private_alias_footprint(alias: &str, generation: u64) -> Footprint {
    let mut footprint = Footprint::default();
    footprint.add(PrivateAccessDescriptor::Alias { alias: alias.to_string(), generation });
    footprint
}

fn private_hosted_footprint(policy_id: &str) -> Footprint {
    let mut footprint = Footprint::default();
    footprint.add(PrivateAccessDescriptor::Hosted { policy_id: policy_id.to_string() });
    footprint
}

fn package_policies(pattern: &str, access: &str) -> PackagePolicies {
    PackagePolicies::new(vec![
        PackagePolicy::new(
            pattern,
            AccessList::parse(access),
            AccessList::parse("$authenticated"),
            AccessList::default(),
        )
        .expect("policy parses"),
    ])
}

fn lockfile(version: &str) -> Lockfile {
    package_lockfile("acme", version)
}

fn package_lockfile(name: &str, version: &str) -> Lockfile {
    let mut packages = serde_json::Map::new();
    packages.insert(
        format!("{name}@{version}"),
        serde_json::json!({
            "resolution": { "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
        }),
    );
    let mut dependencies = serde_json::Map::new();
    dependencies
        .insert(name.to_string(), serde_json::json!({ "specifier": "^1.0.0", "version": version }));
    serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": { "dependencies": dependencies }
        },
        "packages": packages
    }))
    .expect("lockfile parses")
}

fn lockfile_tarball_url(lockfile: &Lockfile, key: &str) -> String {
    let value = serde_json::to_value(lockfile).expect("lockfile serializes");
    value["packages"][key]["resolution"]["tarball"].as_str().expect("tarball URL").to_string()
}

#[derive(Debug)]
struct AllowAllVersions;

impl PackageVersionGuard for AllowAllVersions {
    fn check<'a>(&'a self, _name: &'a str, _version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async { Ok(PackageVersionGuardDecision::Allow) })
    }
}

#[test]
fn resolution_cache_key_normalizes_single_project_requests() {
    let top_level = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    let projects = ResolveRequest {
        projects: Some(vec![ResolveRequestProject {
            dir: ".".to_string(),
            dependencies: deps(&[("foo", "^1.0.0")]),
            ..ResolveRequestProject::default()
        }]),
        ..ResolveRequest::default()
    };

    assert_eq!(
        resolution_cache_key(&config(), &top_level),
        resolution_cache_key(&config(), &projects),
    );
}

#[test]
fn resolution_cache_key_changes_with_dependencies_and_policy() {
    let base = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    let different_dep = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^2.0.0")])),
        ..ResolveRequest::default()
    };
    let different_policy = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        minimum_release_age: Some(60),
        ..ResolveRequest::default()
    };

    let config = config();
    let base_key = resolution_cache_key(&config, &base);

    assert_ne!(base_key, resolution_cache_key(&config, &different_dep));
    assert_ne!(base_key, resolution_cache_key(&config, &different_policy));
}

#[test]
fn resolution_cache_key_hashes_input_lockfile_stably() {
    let first_lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      lodash:
        specifier: ^4.17.21
        version: 4.17.21
      react:
        specifier: ^17.0.2
        version: 17.0.2
",
    )
    .unwrap();
    let reordered_lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      react:
        specifier: ^17.0.2
        version: 17.0.2
      lodash:
        specifier: ^4.17.21
        version: 4.17.21
",
    )
    .unwrap();
    let drifted_lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      lodash:
        specifier: ^4.17.21
        version: 4.17.22
",
    )
    .unwrap();

    let request =
        |lockfile| ResolveRequest { lockfile: Some(lockfile), ..ResolveRequest::default() };
    let config = config();
    let first_key = resolution_cache_key(&config, &request(first_lockfile));
    assert_eq!(first_key, resolution_cache_key(&config, &request(reordered_lockfile)));
    assert_ne!(first_key, resolution_cache_key(&config, &request(drifted_lockfile)));
}

#[test]
fn public_cached_resolution_matches_every_caller() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    let context = RouteContext::from_config(&registry_config());

    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        Footprint::default(),
        b"secret",
        &lockfile,
    ));

    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &Identity::Anonymous,)
            .is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
}

#[test]
fn private_cached_resolution_requires_current_alias_authorization() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp", 1),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config
        .uplinks
        .insert("corp".to_string(), uplink_with_access("https://npm.corp.example/", "alice", 1));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_none(),
    );

    config
        .uplinks
        .insert("corp".to_string(), uplink_with_access("https://npm.corp.example/", "alice", 2));
    let rotated = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &rotated, &user("alice")).is_none(),
    );
}

#[test]
fn same_alias_authorized_users_share_private_resolution_cache() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp", 1),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated", 1),
    );
    let context = RouteContext::from_config(&config);

    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &Identity::Anonymous,)
            .is_none(),
    );
}

#[test]
fn revoked_alias_access_stops_matching_private_resolution_hits() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp", 1),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config
        .uplinks
        .insert("corp".to_string(), uplink_with_access("https://npm.corp.example/", "alice", 1));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );

    config
        .uplinks
        .insert("corp".to_string(), uplink_with_access("https://npm.corp.example/", "bob", 1));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_none(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_some(),
    );
}

#[test]
fn revoked_hosted_package_access_stops_matching_private_resolution_hits() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_hosted_footprint("@private/pkg"),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config.policies = package_policies("@private/*", "alice");
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_none(),
    );

    config.policies = package_policies("@private/*", "bob");
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_none(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_some(),
    );
}

#[test]
fn public_lockfile_routing_keeps_registry_resolutions_compact() {
    let registry = public_registry_config("https://registry.example.test/");
    let router = tarball_router(&registry, Identity::Anonymous);
    let routed = router.route_lockfile(&config(), &lockfile("1.0.0"));
    let value = serde_json::to_value(&routed).expect("lockfile serializes");

    assert_eq!(
        value["packages"]["acme@1.0.0"]["resolution"],
        serde_json::json!({
            "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        }),
    );
}

#[test]
fn private_alias_lockfile_routing_uses_gateway_url() {
    let pacquet_config = config_for_registry("https://npm.corp.example/");
    let mut registry = registry_config();
    registry.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated", 7),
    );
    let router = tarball_router(&registry, user("alice"));

    let routed = router.route_lockfile(&pacquet_config, &lockfile("1.0.0"));
    let tarball = lockfile_tarball_url(&routed, "acme@1.0.0");

    assert!(tarball.starts_with("http://127.0.0.1:7677/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));

    let upstream = router.verification_lockfile(&routed);
    assert_eq!(
        lockfile_tarball_url(&upstream, "acme@1.0.0"),
        "https://npm.corp.example/acme/-/acme-1.0.0.tgz",
    );
}

#[test]
fn private_alias_lockfile_routing_encodes_scoped_packages_as_one_gateway_segment() {
    let pacquet_config = config_for_registry("https://npm.corp.example/");
    let mut registry = registry_config();
    registry.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated", 7),
    );
    let router = tarball_router(&registry, user("alice"));

    let routed = router.route_lockfile(&pacquet_config, &package_lockfile("@acme/foo", "1.0.0"));
    let tarball = lockfile_tarball_url(&routed, "@acme/foo@1.0.0");

    assert!(tarball.contains("/~corp/@acme/foo/-/foo-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));

    let upstream = router.verification_lockfile(&routed);
    assert_eq!(
        lockfile_tarball_url(&upstream, "@acme/foo@1.0.0"),
        "https://npm.corp.example/@acme/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn unknown_lockfile_routing_leaves_resolution_unrewritten() {
    let pacquet_config = config_for_registry("https://unknown.example/");
    let registry = registry_config();
    let router = tarball_router(&registry, user("alice"));

    let input = lockfile("1.0.0");
    let routed = router.route_lockfile(&pacquet_config, &input);

    // An unknown route has no uplink and no managed credential, so pnpr mints
    // no gateway URL: the integrity-only registry resolution is left untouched
    // (the client fetches the upstream tarball directly, as it was resolved
    // anonymously), never rewritten into an explicit tarball URL.
    let value = serde_json::to_value(&routed).expect("lockfile serializes");
    let resolution = &value["packages"]["acme@1.0.0"]["resolution"];
    assert!(
        resolution.get("tarball").is_none(),
        "unknown route stays integrity-only: {resolution}",
    );
    assert_eq!(value, serde_json::to_value(&input).expect("lockfile serializes"));
}

fn lockfile_with_tarball(tarball: &str) -> Lockfile {
    serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": { ".": { "dependencies": { "acme": { "specifier": "^1.0.0", "version": "1.0.0" } } } },
        "packages": {
            "acme@1.0.0": {
                "resolution": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": tarball,
                },
            },
        },
    }))
    .expect("lockfile parses")
}

#[test]
fn reject_off_allowlist_fetches_blocks_unconfigured_hosts() {
    use super::reject_off_allowlist_fetches;
    let context = RouteContext::from_config(&registry_config());

    // The built-in npm registry is allowlisted.
    let ok = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&ok, &context).is_none());

    // An IMDS / off-allowlist default registry is rejected before any fetch.
    let ssrf = ResolveRequest {
        registry: Some("http://169.254.169.254/".to_string()),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&ssrf, &context).is_some());

    // A named registry off the allowlist is rejected too.
    let named = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        named_registries: BTreeMap::from([(
            "@acme".to_string(),
            "http://169.254.169.254/".to_string(),
        )]),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&named, &context).is_some());

    // A semver-range dependency never hits the network, so it is ignored.
    let ranges = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&ranges, &context).is_none());

    // A direct http(s) tarball dependency pointing at an off-allowlist host is
    // rejected before the tarball resolver issues a HEAD/GET.
    let tarball_dep = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        dependencies: Some(deps(&[("foo", "https://169.254.169.254/foo.tgz")])),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&tarball_dep, &context).is_some());

    // A git dependency to an off-allowlist host is rejected the same way.
    let git_dep = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        dependencies: Some(deps(&[("foo", "git+https://169.254.169.254/repo.git#main")])),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&git_dep, &context).is_some());

    // An scp-style git remote (`[user@]host:path`) carries no `://` but still
    // triggers an ssh git fetch, so it is rejected too.
    let scp_dep = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        dependencies: Some(deps(&[("foo", "git@169.254.169.254:org/repo.git")])),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&scp_dep, &context).is_some());

    // Every git transport is gated by origin, not just http(s)/git/ssh — and
    // `file://` (a server-local read) nerf-darts to no host and is rejected.
    for spec in [
        "git+rsync://169.254.169.254/repo",
        "git+ftp://169.254.169.254/repo",
        "git+file:///etc/passwd",
    ] {
        let dep = ResolveRequest {
            registry: Some("https://registry.npmjs.org/".to_string()),
            dependencies: Some(deps(&[("foo", spec)])),
            ..ResolveRequest::default()
        };
        assert!(
            reject_off_allowlist_fetches(&dep, &context).is_some(),
            "spec {spec:?} not rejected",
        );
    }

    // An override whose leaf is an off-allowlist URL is rejected.
    let override_dep = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        overrides: Some(serde_json::json!({ "foo": "https://169.254.169.254/foo.tgz" })),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&override_dep, &context).is_some());
}

#[test]
fn reject_inline_url_auth_scans_input_lockfile_tarballs() {
    use super::reject_inline_url_auth;

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
    let pacquet_config = config_for_registry("https://npm.corp.example/");
    let mut registry = registry_config();
    registry
        .uplinks
        .insert("corp".to_string(), uplink_with_access("https://npm.corp.example/", "alice", 7));
    let router = tarball_router(&registry, user("alice"));
    let routed = router.route_lockfile(&pacquet_config, &lockfile("1.0.0"));

    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp", 7),
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
fn candidate_lists_stay_bounded_and_keep_public_entries() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        Footprint::default(),
        b"secret",
        &lockfile,
    ));
    for index in 0..(MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY + 2) {
        assert!(store_resolution(
            &cache,
            Duration::from_mins(1),
            key.clone(),
            private_alias_footprint(&format!("corp-{index}"), 1),
            b"secret",
            &lockfile,
        ));
    }

    let cache = cache.lock().expect("resolution cache poisoned");
    let candidates = cache.get(&key).expect("base key remains cached");
    assert_eq!(candidates.len(), MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY);
    assert!(candidates.iter().any(|candidate| candidate.footprint.is_public()));
}

#[test]
fn a_package_frame_carries_unpacked_size_and_omits_it_when_unknown() {
    use pacquet_package_manager::{ResolutionObserver, ResolvedPackageHint};

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let registry = public_registry_config("https://r.test/");
    let observer = super::StreamObserver {
        tx,
        package_version_guard: Some(Arc::new(AllowAllVersions)),
        tarball_router: tarball_router(&registry, Identity::Anonymous),
    };

    let hint = |unpacked_size, file_count| ResolvedPackageHint {
        id: "acme@1.0.0",
        name: "acme",
        version: "1.0.0",
        integrity: "sha512-abc",
        tarball_url: "https://r.test/acme/-/acme-1.0.0.tgz",
        unpacked_size,
        file_count,
        from_registry: false,
    };
    observer.on_resolved(hint(Some(123_456), Some(42)));
    observer.on_resolved(hint(None, None));

    let sized: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("sized frame sent")).unwrap();
    assert_eq!(sized["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(sized["fileCount"], serde_json::json!(42));

    let unsized_frame: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("unsized frame sent")).unwrap();
    assert!(unsized_frame.get("unpackedSize").is_none());
    assert!(unsized_frame.get("fileCount").is_none());
    assert_eq!(unsized_frame["tarball"], serde_json::json!("https://r.test/acme/-/acme-1.0.0.tgz"));
}

#[test]
fn package_frames_route_private_alias_tarballs_to_gateway() {
    use pacquet_package_manager::ResolvedPackageHint;

    let mut registry = registry_config();
    registry.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated", 7),
    );
    let router = tarball_router(&registry, user("alice"));
    let frame = super::package_frame(
        &router,
        &ResolvedPackageHint {
            id: "acme@1.0.0",
            name: "acme",
            version: "1.0.0",
            integrity: "sha512-abc",
            tarball_url: "https://npm.corp.example/acme/-/acme-1.0.0.tgz",
            unpacked_size: None,
            file_count: None,
            from_registry: false,
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    assert!(tarball.contains("/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));
}

#[test]
fn package_frame_routes_split_domain_registry_tarball_by_registry() {
    use pacquet_package_manager::ResolvedPackageHint;

    let mut registry = registry_config();
    registry.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated", 7),
    );
    // The package resolves from the private corp registry, but its packument's
    // dist.tarball lives on a *different* host (a split-domain CDN).
    let registries =
        HashMap::from([("default".to_string(), "https://npm.corp.example/".to_string())]);
    let router = tarball_router_with_registries(&registry, user("alice"), registries);
    let frame = super::package_frame(
        &router,
        &ResolvedPackageHint {
            id: "acme@1.0.0",
            name: "acme",
            version: "1.0.0",
            integrity: "sha512-abc",
            tarball_url: "https://cdn.split-domain.example/acme-1.0.0.tgz",
            unpacked_size: None,
            file_count: None,
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
    use pacquet_package_manager::ResolvedPackageHint;

    let registry = registry_config();
    let registries =
        HashMap::from([("default".to_string(), "https://registry.npmjs.org/".to_string())]);
    let router = tarball_router_with_registries(&registry, user("alice"), registries);
    let frame = super::package_frame(
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
            from_registry: true,
        },
    );
    let tarball = frame["tarball"].as_str().expect("tarball URL");

    // The upstream token is never emitted to the client.
    assert_eq!(tarball, "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz", "got {tarball}");
}

#[test]
fn frozen_package_frames_announce_lockfile_tarballs_with_sizes() {
    use pacquet_lockfile::Lockfile;
    use pacquet_resolving_npm_resolver::{DistStats, observed_dist_stats_sink};

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
    let frames = super::frozen_package_frames(
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
    use pacquet_resolving_npm_resolver::observed_dist_stats_sink;

    let pacquet_config = config_for_registry("https://npm.corp.example/");
    let lockfile = lockfile("1.0.0");
    let stats = observed_dist_stats_sink();
    let mut registry = registry_config();
    registry.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated", 7),
    );

    let frames = super::frozen_package_frames(
        &pacquet_config,
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
    use pacquet_lockfile::{LockfileResolution, TarballResolution};

    let tarball = |url: &str, git_hosted: Option<bool>| {
        LockfileResolution::Tarball(TarballResolution {
            tarball: url.to_string(),
            integrity: None,
            git_hosted,
            path: None,
        })
    };

    // `gitHosted: true` must not let a normal https registry tarball opt out.
    assert!(super::is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
        Some(true),
    )));
    // A URL that strict parsing would reject is still scanned when it is http(s).
    assert!(super::is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo 1.0.0.tgz",
        None,
    )));
    // Mutable git-host archive refs are still checked.
    assert!(super::is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/abc123",
        Some(false),
    )));
    // Genuinely git-hosted-by-URL tarballs are skipped regardless of the flag.
    assert!(!super::is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567",
        Some(false),
    )));
    // Non-http schemes are skipped.
    assert!(!super::is_osv_checkable_resolution(&tarball("file:../foo.tgz", None)));
}

#[test]
fn tarball_url_version_extracts_conventional_names_only() {
    use super::tarball_url_version;

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
fn intern_config_caps_distinct_leaked_configs_but_keeps_serving_known_ones() {
    use super::intern_config;
    use pacquet_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-intern-test-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-intern-test-cache");
    let max = 2;

    let request = |registry: &str| ResolveRequest {
        registry: Some(registry.to_string()),
        ..ResolveRequest::default()
    };
    let intern = |registry: &str| {
        intern_config(&configs, &store_dir, &cache_dir, &request(registry), max, usize::MAX)
    };

    // Distinct registry configurations are interned up to the cap.
    assert!(intern("https://a.test/").is_some());
    assert!(intern("https://b.test/").is_some());

    // A new distinct configuration past the cap is refused, not leaked — this
    // is the bound on how much an authenticated caller can make the server
    // leak by varying its registry/policy fields.
    assert!(intern("https://c.test/").is_none());
    // ...and nothing was interned beyond the cap (the refusal didn't leak).
    assert_eq!(configs.lock().expect("config cache poisoned").len(), max);

    // An already-interned configuration is still served even at the cap.
    assert!(intern("https://a.test/").is_some());
}

#[test]
fn intern_config_refuses_a_config_key_larger_than_the_byte_cap() {
    use super::intern_config;
    use pacquet_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-bytecap-test-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-bytecap-test-cache");
    let request = |registry: &str| ResolveRequest {
        registry: Some(registry.to_string()),
        ..ResolveRequest::default()
    };
    let intern = |registry: &str| {
        intern_config(&configs, &store_dir, &cache_dir, &request(registry), 10, 1024)
    };

    // A normal configuration is interned.
    assert!(intern("https://a.test/").is_some());
    // A configuration whose canonical key exceeds the byte cap is refused, so a
    // caller can't amplify the per-config leak with a giant overrides/registry.
    let oversized = format!("https://{}.test/", "x".repeat(2048));
    assert!(intern(&oversized).is_none());
}

#[test]
fn intern_config_keys_overrides_canonically_regardless_of_order() {
    use super::intern_config;
    use pacquet_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-canon-test-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-canon-test-cache");
    let intern = |overrides: serde_json::Value| {
        let request = ResolveRequest { overrides: Some(overrides), ..ResolveRequest::default() };
        intern_config(&configs, &store_dir, &cache_dir, &request, 10, usize::MAX)
    };

    // The same overrides sent with a different JSON key order must dedup to a
    // single interned config — the second call returns the *same* leaked
    // config, not a new one, and the map stays at one entry.
    let first =
        intern(serde_json::json!({ "a": "1.0.0", "b": "2.0.0" })).expect("first config interned");
    let second = intern(serde_json::json!({ "b": "2.0.0", "a": "1.0.0" })).expect("config reused");
    assert!(std::ptr::eq(first, second));
    assert_eq!(configs.lock().expect("config cache poisoned").len(), 1);
}
