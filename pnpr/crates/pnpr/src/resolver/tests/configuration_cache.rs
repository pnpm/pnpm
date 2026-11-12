use super::{
    Arc, HashMap, HashSet, Lockfile, Mutex, PacquetConfig, PathBuf, RegistryDeclaration,
    ResolutionMode, ResolveRequest, ResolveRequestProject, RouteContext, deps, lockfile,
    registry_config, reject_off_allowlist_fetches, resolution_cache_key,
};

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

    assert_eq!(resolution_cache_key(&top_level), resolution_cache_key(&projects));
}

#[test]
fn resolution_cache_key_changes_with_catalogs() {
    let request = |version: &str| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "catalogs": { "default": { "foo": version } }
        }))
        .expect("resolve request parses")
    };

    assert_ne!(resolution_cache_key(&request("^1.0.0")), resolution_cache_key(&request("^2.0.0")));
}

#[test]
fn resolution_cache_key_normalizes_catalog_json_order() {
    let first = serde_json::from_str::<ResolveRequest>(
        r#"{"catalogs":{"tools":{"typescript":"^6","eslint":"^10"},"default":{"react":"^19"}}}"#,
    )
    .expect("first request parses");
    let reordered = serde_json::from_str::<ResolveRequest>(
        r#"{"catalogs":{"default":{"react":"^19"},"tools":{"eslint":"^10","typescript":"^6"}}}"#,
    )
    .expect("reordered request parses");

    assert_eq!(resolution_cache_key(&first), resolution_cache_key(&reordered));
}

#[test]
fn resolution_cache_key_changes_with_project_identity() {
    let request = |name: &str, version: &str| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "projects": [{
                "dir": ".",
                "name": name,
                "version": version,
                "dependencies": { "foo": "^1.0.0" }
            }]
        }))
        .expect("resolve request parses")
    };
    let base = request("app", "1.0.0");
    let renamed = request("renamed-app", "1.0.0");
    let reversioned = request("app", "2.0.0");

    let base_key = resolution_cache_key(&base);
    assert_ne!(base_key, resolution_cache_key(&renamed));
    assert_ne!(base_key, resolution_cache_key(&reversioned));
}

#[test]
fn resolution_cache_key_changes_with_the_project_publish_config() {
    let request = |publish_config: serde_json::Value| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "projects": [{
                "dir": "packages/lib",
                "name": "lib",
                "version": "1.0.0",
                "publishConfig": publish_config
            }]
        }))
        .expect("resolve request parses")
    };
    // An absent publish directory and an explicit one resolve differently, and
    // the two halves of the config are independent: only `linkDirectory`
    // changing must also miss the cache, or a toggled project keeps the link it
    // was resolved with.
    let absent = resolution_cache_key(&request(serde_json::Value::Null));
    let dist = resolution_cache_key(&request(serde_json::json!({ "directory": "dist" })));
    let build = resolution_cache_key(&request(serde_json::json!({ "directory": "build" })));
    let unlinked = resolution_cache_key(&request(
        serde_json::json!({ "directory": "dist", "linkDirectory": false }),
    ));

    assert_ne!(absent, dist);
    assert_ne!(dist, build);
    assert_ne!(dist, unlinked);
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
    let different_mode = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        resolution_mode: ResolutionMode::TimeBased,
        ..ResolveRequest::default()
    };

    let base_key = resolution_cache_key(&base);

    assert_ne!(base_key, resolution_cache_key(&different_dep));
    assert_ne!(base_key, resolution_cache_key(&different_policy));
    assert_ne!(base_key, resolution_cache_key(&different_mode));
}

#[test]
fn resolution_cache_key_changes_with_project_transforms() {
    let request = |patch_hash: &str, extension_version: &str, allow_unused_patches: bool| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "patchedDependencies": { "foo@1.0.0": patch_hash },
            "packageExtensions": {
                "foo@1.0.0": { "dependencies": { "bar": extension_version } }
            },
            "allowUnusedPatches": allow_unused_patches,
        }))
        .expect("resolve request parses")
    };
    let base = request("hash-one", "1.0.0", false);
    let base_key = resolution_cache_key(&base);

    assert_ne!(base_key, resolution_cache_key(&request("hash-two", "1.0.0", false)));
    assert_ne!(base_key, resolution_cache_key(&request("hash-one", "2.0.0", false)));
    assert_ne!(base_key, resolution_cache_key(&request("hash-one", "1.0.0", true)));
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
    let first_key = resolution_cache_key(&request(first_lockfile));
    assert_eq!(first_key, resolution_cache_key(&request(reordered_lockfile)));
    assert_ne!(first_key, resolution_cache_key(&request(drifted_lockfile)));
}

#[test]
fn reject_off_allowlist_fetches_blocks_unconfigured_hosts() {
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

    // A registry the request merely declares is not a fetch target: the client
    // describes its whole configuration, including scopes this resolve never
    // reaches. `RouteHook::allows_fetch` refuses the ones it does reach.
    let declared = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        registries: indexmap::IndexMap::from([(
            "http://169.254.169.254/".to_string(),
            RegistryDeclaration {
                scopes: Some(vec!["@acme".to_string()]),
                ..RegistryDeclaration::default()
            },
        )]),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&declared, &context).is_none());

    // A semver-range dependency never hits the network, so it is ignored.
    let ranges = ResolveRequest {
        registry: Some("https://registry.npmjs.org/".to_string()),
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    assert!(reject_off_allowlist_fetches(&ranges, &context).is_none());

    // A direct HTTP or HTTPS tarball dependency pointing at an off-allowlist host is
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

    // Every git transport is gated by origin, not just HTTP, HTTPS, git, and ssh — and
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
fn intern_config_applies_project_transforms() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-transform-settings-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-transform-settings-cache");
    let request = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "patchedDependencies": { "foo@1.0.0": "abc123" },
        "packageExtensions": {
            "foo@1.0.0": { "dependencies": { "bar": "1.0.0" } }
        },
        "allowUnusedPatches": true,
    }))
    .expect("resolve request parses");

    let config = intern_config(&configs, &store_dir, &cache_dir, &request, 10, usize::MAX)
        .expect("intern config");

    assert!(config.allow_unused_patches);
    assert_eq!(
        config
            .patched_dependency_hashes()
            .expect("read precomputed hashes")
            .expect("patch hashes")["foo@1.0.0"],
        "abc123",
    );
    let extensions = config.package_extensions.as_ref().expect("package extensions");
    assert_eq!(
        extensions["foo@1.0.0"].dependencies.as_ref().expect("extension dependencies")["bar"],
        "1.0.0",
    );
}

#[test]
fn intern_config_uses_lockfile_settings_for_a_legacy_frozen_request() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-lockfile-settings-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-lockfile-settings-cache");
    let request = |settings: Option<pnpm_lockfile::LockfileSettings>| ResolveRequest {
        registry: Some("https://a.test/".to_string()),
        lockfile: Some(Lockfile { settings, ..lockfile("1.0.0") }),
        frozen_lockfile: true,
        ..ResolveRequest::default()
    };
    let intern = |request: &ResolveRequest| {
        intern_config(&configs, &store_dir, &cache_dir, request, 10, usize::MAX)
            .expect("intern config")
    };

    let client_settings = pnpm_lockfile::LockfileSettings {
        auto_install_peers: false,
        dedupe_peers: Some(true),
        exclude_links_from_lockfile: true,
        add_missing_peer_types: Some(true),
        ..pnpm_lockfile::LockfileSettings::default()
    };
    let adopted = intern(&request(Some(client_settings)));
    assert!(!adopted.auto_install_peers);
    assert!(adopted.dedupe_peers);
    assert!(adopted.exclude_links_from_lockfile);
    assert!(adopted.add_missing_peer_types);

    let defaults = intern(&request(None));
    assert!(defaults.auto_install_peers);
    assert!(!defaults.dedupe_peers);
    assert!(!defaults.exclude_links_from_lockfile);
    assert!(!defaults.add_missing_peer_types);
}

#[test]
fn intern_config_prefers_request_settings_and_keys_effective_values() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;

    type Settings = (bool, bool, bool);
    type OptionalSettings = (Option<bool>, Option<bool>, Option<bool>);

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-request-settings-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-request-settings-cache");
    let requested = |(auto, dedupe, exclude): Settings| (Some(auto), Some(dedupe), Some(exclude));
    let request =
        |(auto_install_peers, dedupe_peers, exclude_links_from_lockfile): OptionalSettings,
         lockfile_settings: Option<Settings>,
         frozen_lockfile| {
            let lockfile = lockfile_settings.map(
                |(auto_install_peers, dedupe_peers, exclude_links_from_lockfile)| {
                    serde_json::json!({
                        "lockfileVersion": "9.0",
                        "settings": {
                            "autoInstallPeers": auto_install_peers,
                            "dedupePeers": dedupe_peers,
                            "excludeLinksFromLockfile": exclude_links_from_lockfile,
                        }
                    })
                },
            );
            serde_json::from_value(serde_json::json!({
                "autoInstallPeers": auto_install_peers,
                "dedupePeers": dedupe_peers,
                "excludeLinksFromLockfile": exclude_links_from_lockfile,
                "lockfile": lockfile,
                "frozenLockfile": frozen_lockfile,
            }))
            .expect("resolve request parses")
        };
    let intern = |configs: &Mutex<HashMap<String, super::super::config_cache::CachedConfig>>,
                  request: &ResolveRequest| {
        intern_config(configs, &store_dir, &cache_dir, request, 10, usize::MAX)
            .expect("intern config")
    };
    let effective = |config: &PacquetConfig| {
        (config.auto_install_peers, config.dedupe_peers, config.exclude_links_from_lockfile)
    };
    let legacy = serde_json::from_value::<ResolveRequest>(serde_json::json!({}))
        .expect("legacy resolve request parses");
    assert_eq!(
        (legacy.auto_install_peers, legacy.dedupe_peers, legacy.exclude_links_from_lockfile),
        (None, None, None),
    );

    let first = (false, true, false);
    let second = (true, false, true);
    let frozen = intern(&configs, &request(requested(first), Some(second), true));
    let update = intern(&configs, &request(requested(second), Some(first), false));
    assert_eq!(effective(&frozen), first);
    assert_eq!(effective(&update), second);
    let update_reusing_frozen = intern(&configs, &request(requested(first), Some(second), false));
    assert!(std::sync::Arc::ptr_eq(&frozen, &update_reusing_frozen));

    for (request_settings, lockfile_settings) in [
        ((Some(false), None, None), (true, true, false)),
        ((None, Some(true), None), (false, false, false)),
        ((None, None, Some(false)), (false, true, true)),
    ] {
        let partial = intern(&configs, &request(request_settings, Some(lockfile_settings), true));
        assert!(std::sync::Arc::ptr_eq(&frozen, &partial));
    }
    let partial_update =
        intern(&configs, &request((None, Some(true), None), Some((false, false, true)), false));
    assert_eq!(effective(&partial_update), (true, true, false));

    let mut cache_keys = HashSet::new();
    for auto_install_peers in [false, true] {
        for dedupe_peers in [false, true] {
            for exclude_links_from_lockfile in [false, true] {
                let settings = (auto_install_peers, dedupe_peers, exclude_links_from_lockfile);
                let settings_request = request(requested(settings), None, false);
                let config = intern(&configs, &settings_request);
                assert_eq!(effective(&config), settings);
                let cache_key =
                    resolution_cache_key(&settings_request).expect("resolution cache key");
                assert!(cache_keys.insert(cache_key));
            }
        }
    }
    assert_eq!(
        configs
            .lock()
            .expect("config cache poisoned")
            .len(),
        8,
    );
    assert_eq!(cache_keys.len(), 8);
}

#[test]
fn intern_config_uses_server_defaults_for_a_legacy_update_request() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-update-settings-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-update-settings-cache");
    let request = ResolveRequest {
        registry: Some("https://a.test/".to_string()),
        lockfile: Some(Lockfile {
            settings: Some(pnpm_lockfile::LockfileSettings {
                auto_install_peers: false,
                dedupe_peers: Some(true),
                exclude_links_from_lockfile: true,
                ..pnpm_lockfile::LockfileSettings::default()
            }),
            ..lockfile("1.0.0")
        }),
        frozen_lockfile: false,
        ..ResolveRequest::default()
    };

    let config = intern_config(&configs, &store_dir, &cache_dir, &request, 10, usize::MAX)
        .expect("intern config");
    let defaults = PacquetConfig::new();
    assert_eq!(config.auto_install_peers, defaults.auto_install_peers);
    assert_eq!(config.dedupe_peers, defaults.dedupe_peers);
    assert_eq!(config.exclude_links_from_lockfile, defaults.exclude_links_from_lockfile);
}

#[test]
fn intern_config_ignores_unrelated_lockfile_settings() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-unadopted-settings-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-unadopted-settings-cache");
    let request = |peers_suffix_max_length: u64| ResolveRequest {
        registry: Some("https://a.test/".to_string()),
        frozen_lockfile: true,
        lockfile: Some(Lockfile {
            settings: Some(pnpm_lockfile::LockfileSettings {
                peers_suffix_max_length: Some(peers_suffix_max_length),
                ..pnpm_lockfile::LockfileSettings::default()
            }),
            ..lockfile("1.0.0")
        }),
        ..ResolveRequest::default()
    };
    let intern = |request: &ResolveRequest| {
        intern_config(&configs, &store_dir, &cache_dir, request, 1, usize::MAX)
    };

    let first = intern(&request(1000)).unwrap();
    let second = intern(&request(10)).unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "unrelated lockfile settings must reuse the configuration",
    );
}

#[test]
fn intern_config_evicts_lru_and_releases_inactive_configurations() {
    use std::sync::Arc;
    let configs = Mutex::new(HashMap::new());
    let store_dir = pnpm_store_dir::StoreDir::new(PathBuf::from("/tmp/pnpr-config-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-config-cache");
    let intern = |age| {
        let request = ResolveRequest { minimum_release_age: Some(age), ..Default::default() };
        super::super::intern_config(&configs, &store_dir, &cache_dir, &request, 2, usize::MAX)
            .unwrap()
    };
    let first = intern(1);
    let second = intern(2);
    let weak_second = Arc::downgrade(&second);
    drop(second);
    assert!(Arc::ptr_eq(&first, &intern(1)));
    let third = intern(3);
    assert!(weak_second.upgrade().is_none());
    assert_eq!(configs.lock().unwrap().len(), 2);
    let weak_first = Arc::downgrade(&first);
    for age in 4..1030 {
        assert_eq!(intern(age).minimum_release_age, Some(age));
        assert!(configs.lock().unwrap().len() <= 2);
    }
    assert_eq!(first.minimum_release_age, Some(1));
    assert_eq!(third.minimum_release_age, Some(3));
    drop(first);
    assert!(weak_first.upgrade().is_none());
    assert_eq!(intern(1).minimum_release_age, Some(1));
}

#[test]
fn intern_config_refuses_a_config_key_larger_than_the_byte_cap() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;
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
    // caller can't retain an oversized overrides or registry map.
    let oversized = format!("https://{}.test/", "x".repeat(2048));
    assert!(intern(&oversized).is_none());
}

#[test]
fn intern_config_keys_overrides_canonically_regardless_of_order() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-canon-test-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-canon-test-cache");
    let intern = |overrides: serde_json::Value| {
        let request = ResolveRequest { overrides: Some(overrides), ..ResolveRequest::default() };
        intern_config(&configs, &store_dir, &cache_dir, &request, 10, usize::MAX)
    };

    let first =
        intern(serde_json::json!({ "a": "1.0.0", "b": "2.0.0" })).expect("first config interned");
    let second = intern(serde_json::json!({ "b": "2.0.0", "a": "1.0.0" })).expect("config reused");
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(
        configs
            .lock()
            .expect("config cache poisoned")
            .len(),
        1,
    );
}

/// The client's `resolutionMode` decides which version a pick lands on, so a
/// server resolving on the client's behalf has to run the client's mode
/// rather than its own default — and two modes cannot share one interned
/// config.
#[test]
fn intern_config_resolves_in_the_client_s_resolution_mode() {
    use pnpm_store_dir::StoreDir;

    use super::super::intern_config;

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-resolution-mode-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-resolution-mode-cache");
    let intern = |request: &ResolveRequest| {
        intern_config(&configs, &store_dir, &cache_dir, request, 10, usize::MAX)
            .expect("intern config")
    };

    let legacy = ResolveRequest::default();
    assert_eq!(legacy.resolution_mode, ResolutionMode::Highest);
    assert_eq!(intern(&legacy).resolution_mode, ResolutionMode::Highest);

    for mode in [ResolutionMode::TimeBased, ResolutionMode::LowestDirect] {
        let request = ResolveRequest { resolution_mode: mode, ..ResolveRequest::default() };
        assert_eq!(intern(&request).resolution_mode, mode);
    }

    assert_eq!(
        configs
            .lock()
            .expect("config cache")
            .len(),
        3,
    );
}

#[test]
fn intern_config_keys_add_missing_peer_types() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-peer-types-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-peer-types-cache");
    let request = |add_missing_peer_types| ResolveRequest {
        registry: Some("https://a.test/".to_string()),
        add_missing_peer_types: Some(add_missing_peer_types),
        ..ResolveRequest::default()
    };
    let intern = |request: &ResolveRequest| {
        intern_config(&configs, &store_dir, &cache_dir, request, 10, usize::MAX)
            .expect("intern config")
    };

    assert!(intern(&request(true)).add_missing_peer_types);
    assert!(!intern(&request(false)).add_missing_peer_types);
    assert_ne!(
        resolution_cache_key(&request(true)).expect("resolution cache key"),
        resolution_cache_key(&request(false)).expect("resolution cache key"),
    );
}

#[test]
fn the_server_keys_a_registry_by_the_client_s_prefix() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;

    const REGISTRY: &str = "https://npm.corp.example/";
    let mut client = PacquetConfig::new();
    for prefix in ["zeta", "work"] {
        client.registries_by_prefix.insert(prefix.to_string(), REGISTRY.to_string());
    }
    let request = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "registries": client.registry_declarations(),
    }))
    .expect("resolve request parses");

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-registry-prefix-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-registry-prefix-cache");
    let server = intern_config(&configs, &store_dir, &cache_dir, &request, 10, usize::MAX)
        .expect("intern config");

    assert_eq!(client.declared_registry_prefixes().prefix_for(REGISTRY), Some("work"));
    assert_eq!(server.declared_registry_prefixes().prefix_for(REGISTRY), Some("work"));
    assert_eq!(server.registries_by_prefix, client.registries_by_prefix);
}

#[test]
fn intern_config_keys_project_scoped_optional_peers() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;

    let configs = Mutex::new(HashMap::new());
    let store_dir = StoreDir::new(PathBuf::from("/tmp/pnpr-scoped-peers-store"));
    let cache_dir = PathBuf::from("/tmp/pnpr-scoped-peers-cache");
    let request = |project_scoped_optional_peers| ResolveRequest {
        registry: Some("https://a.test/".to_string()),
        project_scoped_optional_peers: Some(project_scoped_optional_peers),
        ..ResolveRequest::default()
    };
    let intern = |request: &ResolveRequest| {
        intern_config(&configs, &store_dir, &cache_dir, request, 10, usize::MAX)
            .expect("intern config")
    };

    assert!(intern(&request(true)).project_scoped_optional_peers);
    assert!(!intern(&request(false)).project_scoped_optional_peers);
    assert_ne!(
        resolution_cache_key(&request(true)).expect("resolution cache key"),
        resolution_cache_key(&request(false)).expect("resolution cache key"),
    );
}

#[tokio::test]
async fn active_config_admission_follows_detached_work_and_recovers() {
    use pnpr_fixtures::MockConfigPresets;
    let temp = tempfile::tempdir().unwrap();
    let config =
        pnpr_config::Config::proxy("127.0.0.1:7677".parse().unwrap(), temp.path().to_owned());
    let mut runtime = super::super::Resolver::build(&config, None);
    runtime.configs.admission = Arc::new(tokio::sync::Semaphore::new(1));
    let admit = |request: &ResolveRequest| {
        runtime
            .config_for(request)
            .unwrap_or_else(|response| {
                panic!("unexpected admission failure: {}", response.status())
            })
    };
    let first = admit(&ResolveRequest::default());
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        wait.await.unwrap();
        drop(first);
    });
    let second = ResolveRequest { minimum_release_age: Some(42), ..Default::default() };
    let response = runtime
        .config_for(&second)
        .err()
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        runtime.configs.entries
            .lock()
            .unwrap()
            .len(),
        1,
    );
    release.send(()).unwrap();
    task.await.unwrap();
    assert_eq!(admit(&second).minimum_release_age, Some(42));
    let oversized = ResolveRequest {
        overrides: Some(serde_json::json!({"example": "x".repeat(128 * 1024)})),
        ..Default::default()
    };
    assert_eq!(
        runtime
            .config_for(&oversized)
            .err()
            .unwrap()
            .status(),
        axum::http::StatusCode::BAD_REQUEST,
    );
    assert_eq!(admit(&second).minimum_release_age, Some(42));
}

/// A request field that changes the resolution and is in neither key would
/// let one client's cached lockfile answer another's request.
#[test]
fn every_request_field_is_keyed_or_listed_as_unkeyed() {
    use super::super::{cache::RESOLUTION_KEY, config_cache::CONFIG_KEY};

    const UNKEYED_FIELDS: &[&str] = &[
        // These bypass the resolution cache.
        "updatePatches",
        "fixLockfile",
        // The server picks upstream credentials itself.
        "authHeaders",
        // The server renders one lockfile format.
        "acceptedLockfileFormats",
        // Resolution does not read them.
        "ecosystem",
        "nodeVersion",
        "os",
        "arch",
    ];
    let error = serde_json::from_str::<ResolveRequest>(r#"{"\u0000":0}"#)
        .expect_err("an unknown field is refused")
        .to_string();
    let (_, declared) = error.split_once("expected one of ").expect("serde lists the fields");
    let declared: HashSet<&str> = declared
        .split(", ")
        .map(|field| field.trim_matches(|char: char| char == '`' || char.is_whitespace()))
        .map(|field| field.split('`').next().unwrap_or(field))
        .collect();
    let classified: HashSet<&str> = CONFIG_KEY
        .iter()
        .chain(RESOLUTION_KEY)
        .flat_map(|part| part.fields.iter().copied())
        .chain(UNKEYED_FIELDS.iter().copied())
        .collect();

    assert_eq!(declared, classified);
}
