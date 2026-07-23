use super::{
    Add, AddError, node_runtime_version_spec, normalized_save_specifier,
    persist_selected_manifests, prepare_selected_manifests, selected_project_indices,
};
use crate::ResolvedPackages;
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::PinnedVersion;
use pacquet_reporter::{LogEvent, LogLevel, Reporter, SilentReporter};
use pacquet_workspace::Project;
use serde_json::json;
use std::{
    collections::HashSet,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tempfile::tempdir;

const SCOPED_TEST_INTEGRITY: &str = "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==";

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
    let scoped_packument = scoped_registry
        .mock("GET", "/@private%2Ffoo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(scoped_package_body(&scoped_registry_url))
        .expect_at_least(1)
        .create_async()
        .await;

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = format!("{}/", default_registry.url());
    config.registries.insert("@private".to_string(), scoped_registry_url);
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
        pinned_version: PinnedVersion::Patch,
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
    scoped_packument.assert_async().await;
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
    config.catalog_mode = pacquet_config::CatalogMode::Prefer;
    config.minimum_release_age = None;
    let mut servers = Vec::new();
    let mut mocks = Vec::new();
    let mut package_names = Vec::new();

    for (scope, name, response_delay_ms) in packages {
        let package_name = format!("@{scope}/{name}");
        let mut server = mockito::Server::new_async().await;
        let registry_url = format!("{}/", server.url());
        config.registries.insert(format!("@{scope}"), registry_url.clone());

        let response_body = version_body(&package_name, &registry_url);
        let state = Arc::clone(&request_state);
        let latest_path = format!("/@{scope}%2F{name}/latest");
        let latest = server
            .mock("GET", latest_path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_chunked_body(move |writer| {
                let (lock, ready) = &*state;
                let mut requests = lock.lock().unwrap();
                requests.active += 1;
                requests.started += 1;
                requests.max_active = requests.max_active.max(requests.active);
                ready.notify_all();
                let (mut requests, _) = ready
                    .wait_timeout_while(requests, Duration::from_secs(2), |requests| {
                        requests.started < packages.len()
                    })
                    .unwrap();
                drop(requests);
                std::thread::sleep(Duration::from_millis(response_delay_ms));
                requests = lock.lock().unwrap();
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
            .expect_at_least(1)
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
        pinned_version: PinnedVersion::Patch,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<RecordingReporter>()
    .await
    .expect("add should resolve all package selectors");

    {
        let requests = request_state.0.lock().unwrap();
        assert_eq!(
            requests.max_active,
            packages.len(),
            "every selector's latest request should overlap",
        );
    }

    {
        let events = EVENTS.lock().unwrap();
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

    for (latest, packument) in mocks {
        latest.assert_async().await;
        packument.assert_async().await;
    }
    drop(servers);
}

#[tokio::test]
async fn add_reuses_shared_packument_state_for_every_selector_path() {
    #[derive(Default)]
    struct PackumentRequestState {
        active: usize,
        max_active: usize,
        total: usize,
    }

    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();
    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let package_name = "shared-package";
    let mut server = mockito::Server::new_async().await;
    let registry_url = format!("{}/", server.url());
    let response_body = package_body(package_name, &registry_url);
    let request_state = Arc::new(Mutex::new(PackumentRequestState::default()));
    let state = Arc::clone(&request_state);
    let packument = server
        .mock("GET", "/shared-package")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_chunked_body(move |writer| {
            let mut requests = state.lock().unwrap();
            requests.active += 1;
            requests.max_active = requests.max_active.max(requests.active);
            requests.total += 1;
            drop(requests);
            std::thread::sleep(Duration::from_millis(100));
            let result = writer.write_all(response_body.as_bytes());
            state.lock().unwrap().active -= 1;
            result
        })
        .expect(2)
        .create_async()
        .await;

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.cache_dir = dir.path().join("cache");
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = registry_url;
    config.minimum_release_age = Some(24 * 60);
    config.minimum_release_age_exclude = Some(vec![package_name.to_string()]);
    let config = config.leak();

    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    let package_names = [
        "shared-package".to_string(),
        "shared-package@^1.0.0".to_string(),
        "shared-package@^1.0.0".to_string(),
    ];
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
        pinned_version: PinnedVersion::Patch,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<SilentReporter>()
    .await
    .expect("overlapping selectors should share their packument fetch");

    {
        let requests = request_state.lock().unwrap();
        assert_eq!(
            (requests.max_active, requests.total),
            (1, 2),
            "every selector path should share one fetch before the follow-up install fetch",
        );
    }
    packument.assert_async().await;
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
        config.registries.insert(format!("@{scope}"), format!("{}/", server.url()));
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
        pinned_version: PinnedVersion::Patch,
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

#[tokio::test]
async fn add_does_not_wait_for_a_slower_later_resolution_after_an_error() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();
    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let packages = [("first", "a", 100), ("second", "b", 3_000)];
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
        config.registries.insert(format!("@{scope}"), format!("{}/", server.url()));
        let latest_path = format!("/@{scope}%2F{name}/latest");
        let latest = server
            .mock("GET", latest_path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_chunked_body(move |writer| {
                std::thread::sleep(Duration::from_millis(response_delay_ms));
                writer.write_all(b"not valid package metadata")
            })
            .create_async()
            .await;
        package_names.push(package_name);
        mocks.push(latest);
        servers.push(server);
    }

    let config = config.leak();
    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
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
            pinned_version: PinnedVersion::Patch,
            save_catalog_name: None,
            supported_architectures: None,
            lockfile_only: true,
        }
        .run::<SilentReporter>(),
    )
    .await
    .expect("the first selector error should not wait for a slower later selector")
    .expect_err("invalid package metadata should fail resolution");

    match result {
        AddError::ResolveLatest { name, .. } => assert_eq!(name, "@first/a"),
        error => panic!("expected latest-resolution error, got {error:?}"),
    }
    mocks[0].assert_async().await;
    drop(servers);
}

fn scoped_version_body(registry_url: &str) -> String {
    version_body("@private/foo", registry_url)
}

fn version_body(package_name: &str, registry_url: &str) -> String {
    format!(
        r#"{{
  "name": "{package_name}",
  "version": "1.0.0",
  "dist": {{
    "integrity": "{SCOPED_TEST_INTEGRITY}",
    "tarball": "{registry_url}{package_name}/-/package-1.0.0.tgz"
  }}
}}"#,
    )
}

fn scoped_package_body(registry_url: &str) -> String {
    package_body("@private/foo", registry_url)
}

fn package_body(package_name: &str, registry_url: &str) -> String {
    package_body_with_integrity(package_name, registry_url, SCOPED_TEST_INTEGRITY)
}

fn package_body_with_integrity(package_name: &str, registry_url: &str, integrity: &str) -> String {
    format!(
        r#"{{
  "name": "{package_name}",
  "dist-tags": {{ "latest": "1.0.0" }},
  "versions": {{
    "1.0.0": {{
      "name": "{package_name}",
      "version": "1.0.0",
      "dist": {{
        "integrity": "{integrity}",
        "tarball": "{registry_url}{package_name}/-/package-1.0.0.tgz"
      }}
    }}
  }}
}}"#,
    )
}

#[test]
fn normalizes_hosted_git_specifiers_to_shortcut_form() {
    // A bare `owner/repo#committish` shorthand becomes a `github:` shortcut.
    assert_eq!(
        normalized_save_specifier("pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf"),
        "github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf",
    );
    // A full GitHub URL collapses to the same shortcut.
    assert_eq!(
        normalized_save_specifier("https://github.com/pnpm/test-git-fetch"),
        "github:pnpm/test-git-fetch",
    );
    // An explicit `github:` shorthand is idempotent.
    assert_eq!(
        normalized_save_specifier("github:pnpm/test-git-fetch#abc"),
        "github:pnpm/test-git-fetch#abc",
    );
    // GitLab and Bitbucket shorthands and URLs collapse to their own prefixes.
    assert_eq!(normalized_save_specifier("gitlab:owner/repo#abc"), "gitlab:owner/repo#abc");
    assert_eq!(normalized_save_specifier("https://gitlab.com/owner/repo"), "gitlab:owner/repo");
    assert_eq!(normalized_save_specifier("bitbucket:owner/repo#abc"), "bitbucket:owner/repo#abc");
    assert_eq!(
        normalized_save_specifier("https://bitbucket.org/owner/repo"),
        "bitbucket:owner/repo",
    );
    // An auth-bearing HTTPS URL is kept verbatim — the shortcut form cannot
    // carry the embedded credentials, so shortcutting would drop them.
    assert_eq!(
        normalized_save_specifier("git+https://x-access-token:tkn@github.com/foo/bar.git#abc"),
        "git+https://x-access-token:tkn@github.com/foo/bar.git#abc",
    );
    // Non-git specifiers are kept verbatim.
    assert_eq!(normalized_save_specifier("^1.2.3"), "^1.2.3");
    assert_eq!(normalized_save_specifier("npm:bar@^1"), "npm:bar@^1");
    assert_eq!(normalized_save_specifier("file:../bar"), "file:../bar");
    assert_eq!(normalized_save_specifier("workspace:*"), "workspace:*");
}

#[test]
fn node_runtime_version_spec_matches_only_explicit_node_runtime_requests() {
    assert_eq!(node_runtime_version_spec("node", Some("runtime:26")), Some("26"));
    assert_eq!(node_runtime_version_spec("node", Some("runtime:")), Some(""));
    // A registry-range `node` request is owned by the npm resolver.
    assert_eq!(node_runtime_version_spec("node", Some("^26")), None);
    assert_eq!(node_runtime_version_spec("node", None), None);
    // Deno and bun echo the requested spec back, so they save verbatim.
    assert_eq!(node_runtime_version_spec("deno", Some("runtime:2")), None);
    assert_eq!(node_runtime_version_spec("bun", Some("runtime:latest")), None);
}

#[tokio::test]
async fn selected_add_prepares_and_persists_only_selected_projects() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects =
        ["a", "b", "c"].into_iter().map(|name| empty_project(dir.path(), name)).collect::<Vec<_>>();
    let ordered_dirs = [projects[1].root_dir.clone(), projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let config = Box::leak(Box::new(Config::new()));
    let http_client = ThrottledClient::default();
    let http_client_arc = Arc::new(ThrottledClient::default());

    prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &indices,
        &http_client,
        &http_client_arc,
        config,
        None,
        Some(&[DependencyGroup::Prod][..]),
        std::slice::from_ref(&"foo@workspace:*".to_string()),
        PinnedVersion::Major,
        None,
    )
    .await
    .expect("prepare selected manifests");
    persist_selected_manifests::<SilentReporter>(&mut projects, &indices)
        .expect("persist selected manifests");

    assert_eq!(dependency_specifier(&projects[0].manifest, "foo"), Some("workspace:*"));
    assert_eq!(dependency_specifier(&projects[1].manifest, "foo"), Some("workspace:*"));
    assert_eq!(dependency_specifier(&projects[2].manifest, "foo"), None);
    assert_eq!(
        saved_dependency_specifier(&projects[0].manifest, "foo"),
        Some("workspace:*".to_string()),
    );
    assert_eq!(
        saved_dependency_specifier(&projects[1].manifest, "foo"),
        Some("workspace:*".to_string()),
    );
    assert_eq!(saved_dependency_specifier(&projects[2].manifest, "foo"), None);
}

#[tokio::test]
async fn selected_add_merges_catalog_updates_in_command_order() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects = vec![
        project_with_foo(dir.path(), "a", "1.0.0"),
        project_with_foo(dir.path(), "b", "2.0.0"),
    ];
    let ordered_dirs = [projects[1].root_dir.clone(), projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let config = Box::leak(Box::new(Config::new()));
    let http_client = ThrottledClient::default();
    let http_client_arc = Arc::new(ThrottledClient::default());

    let prepared = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &indices,
        &http_client,
        &http_client_arc,
        config,
        None,
        Some(&[DependencyGroup::Prod][..]),
        std::slice::from_ref(&"foo".to_string()),
        PinnedVersion::Major,
        Some("default"),
    )
    .await
    .expect("prepare selected manifests");

    assert_eq!(dependency_specifier(&projects[0].manifest, "foo"), Some("1.0.0"));
    assert_eq!(dependency_specifier(&projects[1].manifest, "foo"), Some("catalog:"));
    assert_eq!(
        prepared
            .updated_catalogs
            .get("default")
            .and_then(|catalog| catalog.get("foo"))
            .map(String::as_str),
        Some("2.0.0"),
    );
    assert_eq!(
        prepared
            .catalogs_override
            .as_ref()
            .and_then(|catalogs| catalogs.get("default"))
            .and_then(|catalog| catalog.get("foo"))
            .map(String::as_str),
        Some("2.0.0"),
    );
}

fn empty_project(root: &std::path::Path, name: &str) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    std::fs::write(&package_json, json!({ "name": name }).to_string()).expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
    }
}

fn project_with_foo(root: &std::path::Path, name: &str, specifier: &str) -> Project {
    let project = empty_project(root, name);
    let mut manifest = project.manifest;
    manifest.add_dependency("foo", specifier, DependencyGroup::Prod).expect("add foo dependency");
    manifest.save().expect("save package.json");
    Project { root_dir: project.root_dir, manifest }
}

fn dependency_specifier<'a>(manifest: &'a PackageManifest, name: &str) -> Option<&'a str> {
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(dependency, _)| *dependency == name)
        .map(|(_, specifier)| specifier)
}

fn saved_dependency_specifier(manifest: &PackageManifest, name: &str) -> Option<String> {
    let saved =
        PackageManifest::from_path(manifest.path().to_path_buf()).expect("reread package.json");
    dependency_specifier(&saved, name).map(str::to_string)
}
