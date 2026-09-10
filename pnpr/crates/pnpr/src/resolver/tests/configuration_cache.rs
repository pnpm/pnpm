use super::{
    BTreeMap, HashMap, HashSet, Lockfile, Mutex, PacquetConfig, PathBuf, RegistryDeclaration,
    ResolutionMode, ResolveRequest, ResolveRequestProject, RouteContext, config, deps, lockfile,
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

    assert_eq!(
        resolution_cache_key(&config(), &top_level),
        resolution_cache_key(&config(), &projects),
    );
}

#[test]
fn resolution_cache_key_changes_with_catalogs() {
    let request = |version: &str| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "catalogs": { "default": { "foo": version } }
        }))
        .expect("resolve request parses")
    };
    let config = config();

    assert_ne!(
        resolution_cache_key(&config, &request("^1.0.0")),
        resolution_cache_key(&config, &request("^2.0.0")),
    );
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

    assert_eq!(
        resolution_cache_key(&config(), &first),
        resolution_cache_key(&config(), &reordered),
    );
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

    let config = config();
    let base_key = resolution_cache_key(&config, &base);
    assert_ne!(base_key, resolution_cache_key(&config, &renamed));
    assert_ne!(base_key, resolution_cache_key(&config, &reversioned));
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

    let config = config();
    let base_key = resolution_cache_key(&config, &base);

    assert_ne!(base_key, resolution_cache_key(&config, &different_dep));
    assert_ne!(base_key, resolution_cache_key(&config, &different_policy));
    assert_ne!(base_key, resolution_cache_key(&config, &different_mode));
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
    let config = config();
    let base = request("hash-one", "1.0.0", false);
    let base_key = resolution_cache_key(&config, &base);

    assert_ne!(base_key, resolution_cache_key(&config, &request("hash-two", "1.0.0", false)));
    assert_ne!(base_key, resolution_cache_key(&config, &request("hash-one", "2.0.0", false)));
    assert_ne!(base_key, resolution_cache_key(&config, &request("hash-one", "1.0.0", true)));
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
        registries: BTreeMap::from([(
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
        config.patched_dependency_hashes().expect("read precomputed hashes").expect("patch hashes")
            ["foo@1.0.0"],
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
        ..pnpm_lockfile::LockfileSettings::default()
    };
    let adopted = intern(&request(Some(client_settings)));
    assert!(!adopted.auto_install_peers);
    assert!(adopted.dedupe_peers);
    assert!(adopted.exclude_links_from_lockfile);

    let defaults = intern(&request(None));
    assert!(defaults.auto_install_peers);
    assert!(!defaults.dedupe_peers);
    assert!(!defaults.exclude_links_from_lockfile);
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
    let intern = |configs: &Mutex<HashMap<String, &'static PacquetConfig>>,
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
    assert_eq!(effective(frozen), first);
    assert_eq!(effective(update), second);
    let update_reusing_frozen = intern(&configs, &request(requested(first), Some(second), false));
    assert!(std::ptr::eq(frozen, update_reusing_frozen));

    for (request_settings, lockfile_settings) in [
        ((Some(false), None, None), (true, true, false)),
        ((None, Some(true), None), (false, false, false)),
        ((None, None, Some(false)), (false, true, true)),
    ] {
        let partial = intern(&configs, &request(request_settings, Some(lockfile_settings), true));
        assert!(std::ptr::eq(frozen, partial));
    }
    let partial_update =
        intern(&configs, &request((None, Some(true), None), Some((false, false, true)), false));
    assert_eq!(effective(partial_update), (true, true, false));

    let intern_key = |settings| intern(&configs, &request(requested(settings), None, false));
    let cache_request = ResolveRequest::default();
    let mut cache_keys = HashSet::new();
    for auto_install_peers in [false, true] {
        for dedupe_peers in [false, true] {
            for exclude_links_from_lockfile in [false, true] {
                let settings = (auto_install_peers, dedupe_peers, exclude_links_from_lockfile);
                let config = intern_key(settings);
                assert_eq!(effective(config), settings);
                let cache_key =
                    resolution_cache_key(config, &cache_request).expect("resolution cache key");
                assert!(cache_keys.insert(cache_key));
            }
        }
    }
    assert_eq!(configs.lock().expect("config cache poisoned").len(), 8);
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

/// Only the three effective fields may reach the interning key. The rest of
/// the `settings` block doesn't change the config, and keying on it would
/// let a caller mint a distinct leaked config per value —
/// `peersSuffixMaxLength` is a `u64` — until `MAX_INTERNED_CONFIGS` is
/// spent and every caller is refused.
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

    assert!(intern(&request(1000)).is_some());
    assert!(
        intern(&request(10)).is_some(),
        "a field the config never reads must not mint a second interned config",
    );
}

#[test]
fn intern_config_caps_distinct_leaked_configs_but_keeps_serving_known_ones() {
    use super::super::intern_config;
    use pnpm_store_dir::StoreDir;
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
    // caller can't amplify the per-config leak with a giant overrides/registry.
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

    // The same overrides sent with a different JSON key order must dedup to a
    // single interned config — the second call returns the *same* leaked
    // config, not a new one, and the map stays at one entry.
    let first =
        intern(serde_json::json!({ "a": "1.0.0", "b": "2.0.0" })).expect("first config interned");
    let second = intern(serde_json::json!({ "b": "2.0.0", "a": "1.0.0" })).expect("config reused");
    assert!(std::ptr::eq(first, second));
    assert_eq!(configs.lock().expect("config cache poisoned").len(), 1);
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

    assert_eq!(configs.lock().expect("config cache").len(), 3);
}
