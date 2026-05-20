use super::InstallPackageFromRegistry;
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_registry_mock::AutoMockInstance;
use pacquet_reporter::{LogEvent, ProgressMessage, Reporter, SilentReporter};
use pacquet_resolving_npm_resolver::{InMemoryPackageMetaCache, NpmResolver};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreDir};
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
        lockfile: false,
        prefer_frozen_lockfile: false,
        skip_runtimes: false,
        offline: false,
        prefer_offline: false,
        lockfile_include_tarball_url: false,
        registry: "https://registry.npmjs.com/".to_string(),
        auto_install_peers: false,
        hoist_workspace_packages: true,
        hoisting_limits: Default::default(),
        external_dependencies: Default::default(),
        dedupe_peer_dependents: false,
        strict_peer_dependencies: false,
        resolve_peers_from_workspace_root: false,
        verify_store_integrity: true,
        side_effects_cache: true,
        side_effects_cache_readonly: false,
        fetch_retries: 2,
        fetch_retry_factor: 10,
        fetch_retry_mintimeout: 10_000,
        fetch_retry_maxtimeout: 60_000,
        workspace_dir: None,
        patched_dependencies: None,
        allow_builds: Default::default(),
        dangerously_allow_all_builds: false,
        scripts_prepend_node_path: Default::default(),
        unsafe_perm: true,
        child_concurrency: 1,
        git_shallow_hosts: pacquet_config::default_git_shallow_hosts(),
        supported_architectures: None,
        ignored_optional_dependencies: None,
        cache_dir: tempdir().unwrap().keep(),
        minimum_release_age: None,
        minimum_release_age_exclude: None,
        minimum_release_age_ignore_missing_time: true,
        minimum_release_age_strict: None,
        trust_policy: Default::default(),
        trust_policy_exclude: None,
        trust_policy_ignore_after: None,
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
        cache_dir: Some(cache_dir.to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: true,
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
    let mock_instance = AutoMockInstance::load_or_init();
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

    InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        logged_methods: &logged_methods,
        requester: "",
        alias: "@pnpm.e2e/hello-world-js-bin",
        resolution: &resolution,
        node_modules_dir: modules_dir.path(),
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let real_name = resolution.id.name.to_string();
    let virtual_store_name = format!("{}@{}", real_name.replace('/', "+"), resolution.id.suffix);
    let virtual_store_path =
        virtual_store_dir.path().join(virtual_store_name).join("node_modules").join(&real_name);
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

    let mock_instance = AutoMockInstance::load_or_init();

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

    InstallPackageFromRegistry {
        tarball_mem_cache: &Default::default(),
        config,
        http_client: &http_client,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: &verified_files_cache,
        logged_methods: &logged_methods,
        requester: "/proj",
        alias: "@pnpm.e2e/hello-world-js-bin",
        resolution: &resolution,
        node_modules_dir: modules_dir.path(),
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
