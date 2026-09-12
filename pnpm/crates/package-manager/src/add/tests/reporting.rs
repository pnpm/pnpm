use super::{
    super::{Add, AddError},
    package_body, scoped_package_body, scoped_version_body, version_body,
};
use crate::{ResolvedPackages, add::specifier::ProtocolSelector};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, SilentReporter};
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tempfile::tempdir;

#[test]
fn jsr_selector_without_a_package_name_reports_the_parser_diagnostic() {
    for input in ["jsr:^1.0.0", "jsr:foo@^1.0.0", "jsr:@scope"] {
        let error = ProtocolSelector::parse(input)
            .err()
            .unwrap_or_else(|| panic!("{input} should be rejected"));
        let surfaced_the_parser_error = matches!(error, AddError::ParseJsrSpecifier(_));
        assert!(
            surfaced_the_parser_error,
            "{input} should surface the jsr parser diagnostic, got {error:?}",
        );
    }
}
#[tokio::test]
async fn add_routes_scoped_packages_to_configured_scoped_registry() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();

    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let mut default_registry = mockito::Server::new_async().await;
    let default_latest = default_registry
        .mock("GET", "/@private%2Ffoo/latest")
        .with_status(500)
        .expect(0)
        .create_async()
        .await;
    let default_packument = default_registry
        .mock("GET", "/@private%2Ffoo")
        .with_status(500)
        .expect(0)
        .create_async()
        .await;

    let mut scoped_registry = mockito::Server::new_async().await;
    let scoped_registry_url = format!("{}/", scoped_registry.url());
    let scoped_latest = scoped_registry
        .mock("GET", "/@private%2Ffoo/latest")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(scoped_version_body(&scoped_registry_url))
        .expect(1)
        .create_async()
        .await;
    // Full metadata races the version endpoint and may be cancelled once the
    // version response supplies everything resolution needs.
    let _scoped_packument = scoped_registry
        .mock("GET", "/@private%2Ffoo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(scoped_package_body(&scoped_registry_url))
        .create_async()
        .await;

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = format!("{}/", default_registry.url());
    config.registries_by_scope.insert("@private".to_string(), scoped_registry_url);
    config.minimum_release_age = None;
    let config = config.leak();

    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    let package_names = ["@private/foo".to_string()];
    Add {
        tarball_mem_cache: Arc::default(),
        resolved_packages: &resolved_packages,
        http_client: &http_client,
        http_client_arc: Arc::new(ThrottledClient::default()),
        config,
        manifest: &mut manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: Some([DependencyGroup::Prod]),
        package_names: &package_names,
        range_spec_style: RangeSpecStyle::Patch,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<SilentReporter>()
    .await
    .expect("add should resolve scoped package through scoped registry");

    default_latest.assert_async().await;
    default_packument.assert_async().await;
    scoped_latest.assert_async().await;
}
#[tokio::test]
async fn add_resolves_package_selectors_concurrently_and_reports_in_selector_order() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    #[derive(Default)]
    struct RequestState {
        active: usize,
        max_active: usize,
        started: usize,
        /// Set once a handler gave up waiting for its peers. Later
        /// handlers then skip the wait, so a genuine loss of concurrency
        /// costs one timeout instead of one per selector.
        barrier_expired: bool,
    }

    // Bounds the barrier so a lost overlap fails instead of hanging.
    // Sized to outlast the scheduling latency of a machine running the
    // whole suite in parallel, since expiry is itself a failure.
    const OVERLAP_BARRIER_TIMEOUT: Duration = Duration::from_secs(15);

    fn start_request(state: &(Mutex<RequestState>, Condvar), expected_requests: usize) {
        let (lock, ready) = state;
        let mut requests = lock.lock().unwrap();
        requests.active += 1;
        requests.started += 1;
        requests.max_active = requests.max_active.max(requests.active);
        ready.notify_all();
        let (mut requests, wait) = ready
            .wait_timeout_while(requests, OVERLAP_BARRIER_TIMEOUT, |requests| {
                requests.started < expected_requests && !requests.barrier_expired
            })
            .unwrap();
        // `wait_timeout_while` re-checks the predicate before it
        // reports a timeout, so `timed_out()` means the peers were
        // still missing when the budget ran out — never that the
        // last one arrived on the deadline.
        if wait.timed_out() {
            requests.barrier_expired = true;
            ready.notify_all();
        }
        drop(requests);
    }

    fn assert_catalog_warning_order(events: &[LogEvent]) {
        let warning_messages: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                LogEvent::Pnpm(log)
                    if log.level == LogLevel::Warn
                        && log.message.starts_with("Catalog version mismatch") =>
                {
                    Some(log.message.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            warning_messages,
            [
                r#"Catalog version mismatch for "@one/a": using direct version "1.0.0" instead of catalog version "9.0.0"."#,
                r#"Catalog version mismatch for "@two/b": using direct version "1.0.0" instead of catalog version "9.0.0"."#,
                r#"Catalog version mismatch for "@three/c": using direct version "1.0.0" instead of catalog version "9.0.0"."#,
            ],
        );
    }

    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(
        project_root.join("pnpm-workspace.yaml"),
        "packages:\n  - '.'\ncatalog:\n  '@one/a': 9.0.0\n  '@two/b': 9.0.0\n  '@three/c': 9.0.0\n",
    )
    .unwrap();
    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let request_state = Arc::new((Mutex::new(RequestState::default()), Condvar::new()));
    let packages = [("one", "a", 200), ("two", "b", 100), ("three", "c", 0)];
    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.catalog_mode = pnpm_config::CatalogMode::Prefer;
    config.minimum_release_age = None;
    let mut servers = Vec::new();
    let mut mocks = Vec::new();
    let mut package_names = Vec::new();

    for (scope, name, response_delay_ms) in packages {
        let package_name = format!("@{scope}/{name}");
        let mut server = mockito::Server::new_async().await;
        let registry_url = format!("{}/", server.url());
        config.registries_by_scope.insert(format!("@{scope}"), registry_url.clone());

        let response_body = version_body(&package_name, &registry_url);
        let state = Arc::clone(&request_state);
        let latest_path = format!("/@{scope}%2F{name}/latest");
        let latest = server
            .mock("GET", latest_path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_chunked_body(move |writer| {
                start_request(&state, packages.len());
                std::thread::sleep(Duration::from_millis(response_delay_ms));
                let mut requests = state.0.lock().unwrap();
                requests.active -= 1;
                drop(requests);
                writer.write_all(response_body.as_bytes())
            })
            .expect(1)
            .create_async()
            .await;
        let packument_path = format!("/@{scope}%2F{name}");
        let packument = server
            .mock("GET", packument_path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(package_body(&package_name, &registry_url))
            .create_async()
            .await;

        package_names.push(package_name);
        mocks.push((latest, packument));
        servers.push(server);
    }

    let config = config.leak();
    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    Add {
        tarball_mem_cache: Arc::default(),
        resolved_packages: &resolved_packages,
        http_client: &http_client,
        http_client_arc: Arc::new(ThrottledClient::default()),
        config,
        manifest: &mut manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: Some([DependencyGroup::Prod]),
        package_names: &package_names,
        range_spec_style: RangeSpecStyle::Patch,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<RecordingReporter>()
    .await
    .expect("add should resolve all package selectors");

    {
        let requests = request_state.0.lock().unwrap();
        assert!(
            !requests.barrier_expired,
            "the selectors' latest requests never overlapped within {OVERLAP_BARRIER_TIMEOUT:?}",
        );
        assert_eq!(
            requests.max_active,
            packages.len(),
            "every selector's latest request should overlap",
        );
    }

    {
        let events = EVENTS.lock().unwrap();
        assert_catalog_warning_order(&events);
    }

    for (latest, _packument) in mocks {
        latest.assert_async().await;
    }
    drop(servers);
}
#[tokio::test]
async fn add_reports_resolution_errors_in_selector_order() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();
    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let packages = [("first", "a", 200), ("second", "b", 0)];
    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.minimum_release_age = None;
    let mut servers = Vec::new();
    let mut mocks = Vec::new();
    let mut package_names = Vec::new();

    for (scope, name, response_delay_ms) in packages {
        let package_name = format!("@{scope}/{name}");
        let mut server = mockito::Server::new_async().await;
        config.registries_by_scope.insert(format!("@{scope}"), format!("{}/", server.url()));
        let latest_path = format!("/@{scope}%2F{name}/latest");
        let latest = server
            .mock("GET", latest_path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_chunked_body(move |writer| {
                std::thread::sleep(Duration::from_millis(response_delay_ms));
                writer.write_all(b"not valid package metadata")
            })
            .expect(1)
            .create_async()
            .await;
        package_names.push(package_name);
        mocks.push(latest);
        servers.push(server);
    }

    let config = config.leak();
    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    let error = Add {
        tarball_mem_cache: Arc::default(),
        resolved_packages: &resolved_packages,
        http_client: &http_client,
        http_client_arc: Arc::new(ThrottledClient::default()),
        config,
        manifest: &mut manifest,
        lockfile: None,
        lockfile_path: None,
        dependency_groups: Some([DependencyGroup::Prod]),
        package_names: &package_names,
        range_spec_style: RangeSpecStyle::Patch,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<SilentReporter>()
    .await
    .expect_err("invalid package metadata should fail resolution");

    match error {
        AddError::ResolveLatest { name, .. } => assert_eq!(name, "@first/a"),
        error => panic!("expected latest-resolution error, got {error:?}"),
    }
    for latest in mocks {
        latest.assert_async().await;
    }
    drop(servers);
}
