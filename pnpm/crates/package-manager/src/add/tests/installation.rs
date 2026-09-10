use super::{
    super::{Add, AddError},
    add_jsr_selector, add_npm_selector, package_body,
};
use crate::{ResolvedPackages, add::specifier::ProtocolSelector};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::SilentReporter;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::tempdir;

#[test]
fn protocol_selector_keys_the_manifest_entry_by_the_name_inside_the_protocol() {
    for (input, expected_name, expected_spec) in [
        ("npm:foo@^1.0.0", "foo", Some("^1.0.0")),
        ("npm:@scope/foo", "@scope/foo", None),
        ("jsr:@scope/foo@^1.0.0", "@scope/foo", Some("jsr:@scope/foo@^1.0.0")),
        ("jsr:@scope/foo", "@scope/foo", Some("jsr:@scope/foo")),
        ("workspace:foo@*", "foo", Some("workspace:foo@*")),
        ("workspace:@scope/foo@^1.0.0", "@scope/foo", Some("workspace:@scope/foo@^1.0.0")),
    ] {
        let selector = ProtocolSelector::parse(input)
            .unwrap_or_else(|error| panic!("{input} should parse: {error}"))
            .unwrap_or_else(|| panic!("{input} should name a package"));
        assert_eq!(selector.package_name(), expected_name);
        assert_eq!(selector.explicit_spec(input), expected_spec);
    }
}
#[test]
fn selectors_that_name_no_package_are_left_to_the_registry_split() {
    for input in [
        // `catalog:` is followed by a catalog name, never a package name.
        "catalog:",
        "catalog:default",
        "catalog:@scope/foo",
        // Every `workspace:` form that is a range or a path rather than a
        // name, Windows drive letters and UNC shares included.
        "workspace:*",
        "workspace:^1.0.0",
        "workspace:./pkg",
        "workspace:../pkg",
        "workspace:/tmp/pkg",
        "workspace:~/pkg",
        r"workspace:C:\repo\pkg",
        "workspace:C:/repo/pkg",
        r"workspace:\\server\pkg",
        // Plain registry selectors.
        "foo",
        "@scope/foo@^1.0.0",
    ] {
        let parsed = ProtocolSelector::parse(input)
            .unwrap_or_else(|error| panic!("{input} should parse: {error}"));
        let names_no_package = parsed.is_none();
        assert!(names_no_package, "{input} should not be read as a protocol selector");
    }
}
#[test]
fn protocol_selector_rejects_a_name_no_dependency_can_be_declared_under() {
    for input in ["npm:", "npm:@", "npm:../evil"] {
        let error = ProtocolSelector::parse(input)
            .err()
            .unwrap_or_else(|| panic!("{input} should be rejected"));
        let rejected_the_name = matches!(error, AddError::InvalidPackageName { .. });
        assert!(
            rejected_the_name,
            "{input} should be rejected as an invalid package name, got {error:?}",
        );
    }
}
#[tokio::test]
async fn add_saves_a_jsr_selector_under_its_jsr_name_with_the_picked_version_pinned() {
    assert_eq!(add_jsr_selector("jsr:@pnpm-e2e/foo").await, Some("jsr:^1.0.0".to_string()));
}
#[tokio::test]
async fn add_saves_an_npm_selector_without_a_version_at_the_default_pin() {
    assert_eq!(add_npm_selector("npm:foo").await, Some("^1.0.0".to_string()));
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
        range_spec_style: RangeSpecStyle::Patch,
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
async fn add_does_not_wait_for_a_slower_later_resolution_after_an_error() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();
    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    // The gap between the two delays is what the deadline below reads:
    // the failing selector must return well inside it while the slower
    // peer is still sleeping, with enough slack that a loaded machine
    // cannot push the fast path past the deadline.
    let packages = [("first", "a", 100), ("second", "b", 20_000)];
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
        Duration::from_secs(5),
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
/// A signed tarball URL carries its token in the query string, which
/// `redact_and_sanitize` keeps — and `reqwest` echoes the request URL back
/// in its own message, so the cause has to be scrubbed as well as the
/// specifier.
#[test]
fn a_tarball_error_chain_drops_url_secrets() {
    for url in [
        "https://example.com/pkg.tgz?token=SIGNEDSECRET",
        "https://alice:hunter2@example.com/pkg.tgz",
        "https://alice:hunter2@example.com/pkg.tgz?token=SIGNEDSECRET#frag",
        // The HEAD request follows redirects, so the URL a failure names
        // need not be the one the command was given.
        "https://storage.example.net/blob?X-Amz-Signature=SIGNEDSECRET",
    ] {
        // The shape `reqwest` renders for a transport failure, whose leaf
        // frames carry the reason and whose first frame carries the URL.
        let rendered = crate::add::aliasless::redacted_error_chain(&std::io::Error::other(
            format!("error sending request for url ({url}): tcp connect error"),
        ));
        eprintln!("RENDERED: {rendered}");
        assert!(!rendered.contains("SIGNEDSECRET"), "query token must not survive: {rendered}");
        assert!(!rendered.contains("hunter2"), "password must not survive: {rendered}");
        assert!(
            rendered.contains("example") && rendered.contains("tcp connect error"),
            "the host and the reason must survive: {rendered}",
        );
    }
}
/// A password containing `?` or `#` defeats the userinfo scan — it reads the
/// `?` as the end of the authority and leaves `user:pa?ss@host` in place —
/// so cutting the URL there would publish the password's prefix. Such a
/// token fails closed rather than being shortened.
#[test]
fn a_url_whose_userinfo_survives_the_scan_is_hidden_entirely() {
    for url in [
        "https://alice:hunter2?x@example.com/pkg.tgz",
        "https://alice:hunter2#x@example.com/pkg.tgz",
    ] {
        let rendered = crate::add::aliasless::redacted_error_chain(&std::io::Error::other(
            format!("error sending request for url ({url}): tcp connect error"),
        ));
        eprintln!("RENDERED: {rendered}");
        assert!(!rendered.contains("hunter"), "no part of the password may survive: {rendered}");
        assert!(rendered.contains("[hidden]"), "the URL must fail closed: {rendered}");
        assert!(rendered.contains("tcp connect error"), "the reason must survive: {rendered}");
    }
}
