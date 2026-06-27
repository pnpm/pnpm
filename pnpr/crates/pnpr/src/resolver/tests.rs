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
    config::{Config as RegistryConfig, UpstreamAlias},
    policy::{AccessList, Identity},
    route::{Footprint, PrivateAccessDescriptor, RouteContext},
};

fn config() -> PacquetConfig {
    let mut config = PacquetConfig::new();
    config.registry = "https://registry.example.test/".to_string();
    config
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

fn user(name: &str) -> Identity {
    Identity::User { username: name.to_string() }
}

fn alias(registry: &str, access: &str, generation: u64) -> UpstreamAlias {
    UpstreamAlias {
        registry: registry.to_string(),
        package: None,
        authorization: "Bearer alias-secret".to_string(),
        access: AccessList::parse(access),
        generation,
    }
}

fn private_alias_footprint(alias: &str, generation: u64) -> Footprint {
    let mut footprint = Footprint::default();
    footprint.add(PrivateAccessDescriptor::Alias { alias: alias.to_string(), generation });
    footprint
}

fn lockfile(version: &str) -> Lockfile {
    let mut packages = serde_json::Map::new();
    packages.insert(
        format!("acme@{version}"),
        serde_json::json!({
            "resolution": { "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
        }),
    );
    serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": { "dependencies": { "acme": { "specifier": "^1.0.0", "version": version } } }
        },
        "packages": packages
    }))
    .expect("lockfile parses")
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
        .upstream_aliases
        .insert("corp".to_string(), alias("https://npm.corp.example/", "alice", 1));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_none(),
    );

    config
        .upstream_aliases
        .insert("corp".to_string(), alias("https://npm.corp.example/", "alice", 2));
    let rotated = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &rotated, &user("alice")).is_none(),
    );
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
    let observer =
        super::StreamObserver { tx, package_version_guard: Some(Arc::new(AllowAllVersions)) };

    let hint = |unpacked_size, file_count| ResolvedPackageHint {
        id: "acme@1.0.0",
        name: "acme",
        version: "1.0.0",
        integrity: "sha512-abc",
        tarball_url: "https://r.test/acme/-/acme-1.0.0.tgz",
        unpacked_size,
        file_count,
    };
    observer.on_resolved(hint(Some(123_456), Some(42)));
    observer.on_resolved(hint(None, None));

    let sized: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("sized frame sent")).unwrap();
    assert_eq!(sized["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(sized["fileCount"], serde_json::json!(42));

    let unsized_frame: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("unsized frame sent")).unwrap();
    dbg!(&unsized_frame);
    assert!(unsized_frame.get("unpackedSize").is_none());
    assert!(unsized_frame.get("fileCount").is_none());
    assert_eq!(unsized_frame["tarball"], serde_json::json!("https://r.test/acme/-/acme-1.0.0.tgz"));
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

    let frames = super::frozen_package_frames(&config(), &lockfile, &stats);
    dbg!(frames.len());
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
