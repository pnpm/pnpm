#![expect(
    clippy::default_trait_access,
    reason = "struct-literal test fixtures; field types are evident from the literal and naming each would force ~20 imports"
)]

use super::{InstallPackageFromRegistry, InstallPackageFromRegistryError};
use pacquet_config::Config;
use pacquet_lockfile::{LockfileResolution, TarballResolution};
use pacquet_network::{RetryOpts, ThrottledClient};
use pacquet_reporter::{LogEvent, ProgressMessage, Reporter, SilentReporter};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, ResolveResult, Resolver, WantedDependency};
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreDir};
use pacquet_testing_utils::registry::TestRegistry;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicU8},
};
use tempfile::tempdir;

fn create_config(store_dir: &Path, modules_dir: &Path, virtual_store_dir: &Path) -> Config {
    Config {
        hoist: false,
        hoist_pattern: None,
        public_hoist_pattern: None,
        shamefully_hoist: false,
        store_dir: StoreDir::new(store_dir),
        modules_dir: modules_dir.to_path_buf(),
        node_linker: Default::default(),
        symlink: false,
        virtual_store_dir: virtual_store_dir.to_path_buf(),
        enable_global_virtual_store: false,
        global_virtual_store_dir: virtual_store_dir.to_path_buf(),
        package_import_method: Default::default(),
        modules_cache_max_age: 0,
        virtual_store_dir_max_length: pacquet_config::default_virtual_store_dir_max_length(),
        peers_suffix_max_length: pacquet_config::default_peers_suffix_max_length(),
        lockfile: false,
        prefer_frozen_lockfile: false,
        optimistic_repeat_install: false,
        skip_runtimes: false,
        offline: false,
        prefer_offline: false,
        lockfile_include_tarball_url: false,
        registry: "https://registry.npmjs.com/".to_string(),
        pnpr_server: None,
        named_registries: Default::default(),
        auto_install_peers: false,
        auto_install_peers_from_highest_match: false,
        exclude_links_from_lockfile: false,
        hoist_workspace_packages: true,
        hoisting_limits: Default::default(),
        link_workspace_packages: Default::default(),
        inject_workspace_packages: false,
        prefer_workspace_packages: false,
        external_dependencies: Default::default(),
        dedupe_peer_dependents: false,
        dedupe_peers: false,
        dedupe_direct_deps: true,
        dedupe_injected_deps: false,
        strict_peer_dependencies: false,
        resolve_peers_from_workspace_root: false,
        block_exotic_subdeps: false,
        verify_store_integrity: true,
        side_effects_cache: true,
        side_effects_cache_readonly: false,
        fetch_retries: 2,
        fetch_retry_factor: 10,
        fetch_retry_mintimeout: 10_000,
        fetch_retry_maxtimeout: 60_000,
        network_concurrency: pacquet_network::default_network_concurrency(),
        fetch_timeout: 60_000,
        user_agent: "pnpm".to_string(),
        npmrc_auth_file: None,
        workspace_dir: None,
        patched_dependencies: None,
        config_dependencies: None,
        allow_builds: Default::default(),
        dangerously_allow_all_builds: false,
        scripts_prepend_node_path: Default::default(),
        enable_pre_post_scripts: false,
        script_shell: None,
        node_options: None,
        extra_bin_paths: Default::default(),
        unsafe_perm: true,
        child_concurrency: 1,
        workspace_concurrency: 1,
        recursive: false,
        filter: Vec::new(),
        filter_prod: Vec::new(),
        git_shallow_hosts: pacquet_config::default_git_shallow_hosts(),
        supported_architectures: None,
        ignored_optional_dependencies: None,
        overrides: None,
        package_extensions: None,
        cache_dir: tempdir().unwrap().keep(),
        dlx_cache_max_age: 24 * 60,
        minimum_release_age: None,
        minimum_release_age_exclude: None,
        minimum_release_age_ignore_missing_time: true,
        minimum_release_age_strict: None,
        trust_lockfile: false,
        trust_policy: Default::default(),
        trust_policy_exclude: None,
        trust_policy_ignore_after: None,
        resolution_mode: Default::default(),
        catalog_mode: Default::default(),
        catalogs: None,
        save_catalog_name: None,
        registry_supports_time_field: false,
        allowed_deprecated_versions: Default::default(),
        update_config: Default::default(),
        peer_dependency_rules: Default::default(),
        auth_headers: Default::default(),
        proxy: Default::default(),
        tls: Default::default(),
        tls_by_uri: Default::default(),
    }
}

async fn resolve_via_mock(
    registry: &str,
    cache_dir: &Path,
    http_client: Arc<ThrottledClient>,
    alias: &str,
    range: &str,
) -> pacquet_resolving_resolver_base::ResolveResult {
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    let resolver = NpmResolver {
        registries,
        named_registries: HashMap::new(),
        http_client,
        auth_headers: Default::default(),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(cache_dir.to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: true,
        full_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let wanted = WantedDependency {
        alias: Some(alias.to_string()),
        bare_specifier: Some(range.to_string()),
        ..WantedDependency::default()
    };
    resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolve succeeds against the mock registry")
        .expect("resolver claims the dep")
}

#[tokio::test]
pub async fn should_install_package_from_pre_resolved_result() {
    let mock_instance = TestRegistry::start();
    let store_dir = tempdir().unwrap();
    let modules_dir = tempdir().unwrap();
    let virtual_store_dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    let mut config = create_config(store_dir.path(), modules_dir.path(), virtual_store_dir.path());
    config.registry = mock_instance.url();
    let config: &'static Config = config.pipe(Box::new).pipe(Box::leak);

    let http_client = Arc::new(ThrottledClient::new_for_installs());
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let logged_methods = AtomicU8::new(0);

    let resolution = resolve_via_mock(
        &config.registry,
        cache_dir.path(),
        Arc::clone(&http_client),
        "@pnpm.e2e/hello-world-js-bin",
        "1.0.0",
    )
    .await;

    let name_ver = resolution.name_ver.as_ref().expect("npm resolver fills name_ver");
    let real_name = name_ver.name.to_string();
    let virtual_store_name = format!("{}@{}", real_name.replace('/', "+"), name_ver.suffix);
    let slot_dir = virtual_store_dir.path().join(&virtual_store_name);

    InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        prefetched_cas_paths: None,
        logged_methods: &logged_methods,
        requester: "",
        alias: "@pnpm.e2e/hello-world-js-bin",
        resolution: &resolution,
        node_modules_dir: modules_dir.path(),
        slot_dir: &slot_dir,
        first_visit: true,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let virtual_store_path = slot_dir.join("node_modules").join(&real_name);
    assert!(virtual_store_path.is_dir());

    // Make sure the symlink resolves to the correct path. pacquet
    // writes the contents as a path relative to the link's parent
    // (matching upstream `symlink-dir`), so canonicalize via the
    // link itself rather than comparing `read_link` output against
    // the absolute store path.
    let symlink_path = modules_dir.path().join("@pnpm.e2e/hello-world-js-bin");
    assert_eq!(
        dunce::canonicalize(&symlink_path).expect("canonicalize symlink"),
        dunce::canonicalize(&virtual_store_path).expect("canonicalize virtual store path"),
    );

    drop((store_dir, modules_dir, virtual_store_dir, cache_dir, mock_instance));
}

/// Second-edge install for the same `(name, version)` must NOT emit
/// `pnpm:progress resolved` or `pnpm:progress imported` — those are
/// per-package signals upstream, not per-edge. The second visitor
/// only refreshes the per-parent symlink. Pin the contract here so a
/// future refactor that moves the gate can't quietly reintroduce
/// per-edge spam.
#[tokio::test]
async fn second_visit_skips_progress_emits_but_still_links() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let mock_instance = TestRegistry::start();
    let store_dir = tempdir().unwrap();
    let modules_dir = tempdir().unwrap();
    let second_parent_dir = tempdir().unwrap();
    let virtual_store_dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    let mut config = create_config(store_dir.path(), modules_dir.path(), virtual_store_dir.path());
    config.registry = mock_instance.url();
    let config: &'static Config = config.pipe(Box::new).pipe(Box::leak);

    let http_client = Arc::new(ThrottledClient::new_for_installs());
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let logged_methods = AtomicU8::new(0);

    let resolution = resolve_via_mock(
        &config.registry,
        cache_dir.path(),
        Arc::clone(&http_client),
        "@pnpm.e2e/hello-world-js-bin",
        "1.0.0",
    )
    .await;

    let name_ver = resolution.name_ver.as_ref().expect("npm resolver fills name_ver");
    let real_name = name_ver.name.to_string();
    let virtual_store_name = format!("{}@{}", real_name.replace('/', "+"), name_ver.suffix);
    let slot_dir = virtual_store_dir.path().join(&virtual_store_name);

    // First edge: full path. Run, then clear events for the assertion
    // on the second edge.
    InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        prefetched_cas_paths: None,
        logged_methods: &logged_methods,
        requester: "/proj",
        alias: "first-alias",
        resolution: &resolution,
        node_modules_dir: modules_dir.path(),
        slot_dir: &slot_dir,
        first_visit: true,
    }
    .run::<RecordingReporter>()
    .await
    .expect("first visit installs cleanly");
    EVENTS.lock().unwrap().clear();

    // Second edge: same `(name, version)`, different parent dir.
    InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        prefetched_cas_paths: None,
        logged_methods: &logged_methods,
        requester: "/proj",
        alias: "second-alias",
        resolution: &resolution,
        node_modules_dir: second_parent_dir.path(),
        slot_dir: &slot_dir,
        first_visit: false,
    }
    .run::<RecordingReporter>()
    .await
    .expect("second visit symlinks cleanly");

    let kinds: Vec<&'static str> = EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            LogEvent::Progress(log) => Some(match &log.message {
                ProgressMessage::Resolved { .. } => "resolved",
                ProgressMessage::Fetched { .. } => "fetched",
                ProgressMessage::FoundInStore { .. } => "found_in_store",
                ProgressMessage::Imported { .. } => "imported",
            }),
            _ => None,
        })
        .collect();
    assert!(kinds.is_empty(), "second visit must not emit progress events, got {kinds:?}");

    // The second-parent symlink must exist after the call.
    let symlink_path = second_parent_dir.path().join("second-alias");
    assert!(symlink_path.exists() || symlink_path.is_symlink(), "per-parent symlink missing");

    drop((store_dir, modules_dir, second_parent_dir, virtual_store_dir, cache_dir, mock_instance));
}

/// `InstallPackageFromRegistry::run` emits the `pnpm:progress` per-
/// package sequence: `resolved` before the tarball download, then
/// `fetched` (or `found_in_store` on a cache hit) from inside
/// `DownloadTarballToStore`, then `imported` after `create_cas_files`
/// returns Ok. Pin the order with a recording reporter — a regression
/// in either the sequence or the `package_id`/`requester` payload
/// would currently slip through since the tarball-side and
/// frozen-lockfile-side tests don't exercise this code path.
#[tokio::test]
async fn install_emits_progress_sequence() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let mock_instance = TestRegistry::start();

    let store_dir = tempdir().unwrap();
    let modules_dir = tempdir().unwrap();
    let virtual_store_dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();

    let mut config = create_config(store_dir.path(), modules_dir.path(), virtual_store_dir.path());
    config.registry = mock_instance.url();
    let config: &'static Config = config.pipe(Box::new).pipe(Box::leak);

    let http_client = Arc::new(ThrottledClient::new_for_installs());
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let logged_methods = AtomicU8::new(0);

    let resolution = resolve_via_mock(
        &config.registry,
        cache_dir.path(),
        Arc::clone(&http_client),
        "@pnpm.e2e/hello-world-js-bin",
        "1.0.0",
    )
    .await;

    let name_ver = resolution.name_ver.as_ref().expect("npm resolver fills name_ver");
    let real_name = name_ver.name.to_string();
    let virtual_store_name = format!("{}@{}", real_name.replace('/', "+"), name_ver.suffix);
    let slot_dir = virtual_store_dir.path().join(&virtual_store_name);

    InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        prefetched_cas_paths: None,
        logged_methods: &logged_methods,
        requester: "/proj",
        alias: "@pnpm.e2e/hello-world-js-bin",
        resolution: &resolution,
        node_modules_dir: modules_dir.path(),
        slot_dir: &slot_dir,
        first_visit: true,
    }
    .run::<RecordingReporter>()
    .await
    .expect("install should succeed against the mock registry");

    let progress: Vec<ProgressMessage> = EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            LogEvent::Progress(log) => Some(log.message.clone()),
            _ => None,
        })
        .collect();

    // Order: resolved → fetched (or found_in_store on a warm rerun)
    // → imported. The mock store is a tempdir, so the first install
    // always goes through the network path → `Fetched`. Pin the
    // shape so a future re-ordering breaks the test.
    let kinds: Vec<&'static str> = progress
        .iter()
        .map(|message| match message {
            ProgressMessage::Resolved { .. } => "resolved",
            ProgressMessage::Fetched { .. } => "fetched",
            ProgressMessage::FoundInStore { .. } => "found_in_store",
            ProgressMessage::Imported { .. } => "imported",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["resolved", "fetched", "imported"],
        "unexpected progress sequence: {progress:?}",
    );

    // Pin the (`package_id`, `requester`) on the resolved event —
    // the install layer threads `requester` here as the install
    // root; `package_id` is `{name}@{version}` once the version is
    // resolved.
    match &progress[0] {
        ProgressMessage::Resolved { package_id, requester } => {
            assert_eq!(package_id, "@pnpm.e2e/hello-world-js-bin@1.0.0");
            assert_eq!(requester, "/proj");
        }
        other => panic!("first event must be Resolved; got {other:?}"),
    }

    drop((store_dir, modules_dir, virtual_store_dir, cache_dir, mock_instance));
}

/// Regression test: a `ResolveResult` whose `name_ver` is `None`
/// (every non-npm resolver — git / tarball / local) must surface as
/// [`InstallPackageFromRegistryError::UnsupportedResolution`] rather
/// than panicking. Pins the install path's contract once the git
/// resolver is wired into the chain.
#[tokio::test]
async fn install_returns_unsupported_resolution_when_name_ver_missing() {
    let store_dir = tempdir().unwrap();
    let modules_dir = tempdir().unwrap();
    let virtual_store_dir = tempdir().unwrap();

    let config = create_config(store_dir.path(), modules_dir.path(), virtual_store_dir.path());
    let config: &'static Config = config.pipe(Box::new).pipe(Box::leak);

    let http_client = Arc::new(ThrottledClient::new_for_installs());
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let logged_methods = AtomicU8::new(0);

    let resolution = ResolveResult {
        id: "git+ssh://git@example.com/foo/bar.git#deadbeef".into(),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: None,
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://example.com/foo.tar.gz".to_string(),
            integrity: None,
            git_hosted: Some(true),
            path: None,
        }),
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: Some("github:foo/bar#deadbeef".to_string()),
        alias: Some("bar".to_string()),
        policy_violation: None,
    };

    let slot_dir = virtual_store_dir.path().join("bar@unused");

    let result = InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        prefetched_cas_paths: None,
        logged_methods: &logged_methods,
        requester: "",
        alias: "bar",
        resolution: &resolution,
        node_modules_dir: modules_dir.path(),
        slot_dir: &slot_dir,
        first_visit: true,
    }
    .run::<SilentReporter>()
    .await;

    match result {
        Err(InstallPackageFromRegistryError::UnsupportedResolution { detail }) => {
            assert!(
                detail.contains("git-repository"),
                "error should name the resolver tag: {detail}",
            );
        }
        other => panic!("expected UnsupportedResolution, got {other:?}"),
    }

    drop((store_dir, modules_dir, virtual_store_dir));
}
