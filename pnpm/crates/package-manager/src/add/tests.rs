mod links;

mod catalogs;

mod runtimes;

mod resolution;

mod reporting;

mod installation;

mod workspace;

use super::{Add, AddOwned, AddView};
use crate::ResolvedPackages;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::SilentReporter;
use pnpm_workspace::Project;
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

const SCOPED_TEST_INTEGRITY: &str = "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==";

/// The add inputs a manifest-preparation test varies, saving into
/// `dependencies` with the major range style and no lockfile.
fn test_add<'a>(
    config: &'static Config,
    http_client: &'a ThrottledClient,
    package_names: &'a [String],
    save_catalog_name: Option<&str>,
) -> (AddView<'a>, AddOwned) {
    (
        AddView {
            resolved_packages: Box::leak(Box::new(ResolvedPackages::default())),
            http_client,
            config,
            lockfile: None,
            lockfile_path: None,
            package_names,
            range_spec_style: RangeSpecStyle::Major,
            lockfile_only: false,
        },
        AddOwned {
            tarball_mem_cache: Arc::new(pnpm_tarball::MemCache::default()),
            http_client_arc: Arc::new(ThrottledClient::default()),
            dependency_groups: Some(vec![DependencyGroup::Prod]),
            save_catalog_name: save_catalog_name.map(str::to_string),
            supported_architectures: None,
        },
    )
}

async fn add_npm_selector(selector: &str) -> Option<String> {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();

    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let mut registry = mockito::Server::new_async().await;
    let registry_url = format!("{}/", registry.url());
    let _latest = registry
        .mock("GET", "/foo/latest")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(version_body("foo", &registry_url))
        .create_async()
        .await;
    let _packument = registry
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry_url))
        .create_async()
        .await;

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = registry_url;
    config.minimum_release_age = None;
    let config = config.leak();

    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    let package_names = [selector.to_string()];
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
        range_spec_style: RangeSpecStyle::Major,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_or_else(|error| panic!("add {selector} should succeed: {error}"));

    saved_dependency_specifier(&manifest, "foo")
}

/// Run `pacquet add <selector>` against a mocked `@jsr` registry and report
/// the specifier the manifest ends up with. The default registry is mocked
/// too, expecting no request: a JSR package must be looked up under the
/// `@jsr` scope, never on the registry the project defaults to.
async fn add_jsr_selector(selector: &str) -> Option<String> {
    const JSR_NPM_NAME: &str = "@jsr/pnpm-e2e__foo";

    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    std::fs::create_dir_all(&project_root).unwrap();

    let mut manifest = PackageManifest::create_if_needed(project_root.join("package.json"))
        .expect("create manifest");

    let mut default_registry = mockito::Server::new_async().await;
    let default_requests =
        default_registry.mock("GET", mockito::Matcher::Any).expect(0).create_async().await;

    let mut jsr_registry = mockito::Server::new_async().await;
    let jsr_registry_url = format!("{}/", jsr_registry.url());
    let _jsr_latest = jsr_registry
        .mock("GET", "/@jsr%2Fpnpm-e2e__foo/latest")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(version_body(JSR_NPM_NAME, &jsr_registry_url))
        .create_async()
        .await;
    let _jsr_packument = jsr_registry
        .mock("GET", "/@jsr%2Fpnpm-e2e__foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body(JSR_NPM_NAME, &jsr_registry_url))
        .create_async()
        .await;

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = format!("{}/", default_registry.url());
    config.registries_by_scope.insert("@jsr".to_string(), jsr_registry_url);
    config.minimum_release_age = None;
    let config = config.leak();

    let http_client = ThrottledClient::default();
    let resolved_packages = ResolvedPackages::default();
    let package_names = [selector.to_string()];
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
        range_spec_style: RangeSpecStyle::Major,
        save_catalog_name: None,
        supported_architectures: None,
        lockfile_only: true,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_or_else(|error| panic!("add {selector} should succeed: {error}"));

    default_requests.assert_async().await;
    saved_dependency_specifier(&manifest, "@pnpm-e2e/foo")
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

fn empty_project(root: &std::path::Path, name: &str) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    std::fs::write(&package_json, json!({ "name": name }).to_string()).expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
        dependency_manifest: None,
    }
}

fn project_with_foo(root: &std::path::Path, name: &str, specifier: &str) -> Project {
    let project = empty_project(root, name);
    let mut manifest = project.manifest;
    manifest.add_dependency("foo", specifier, DependencyGroup::Prod).expect("add foo dependency");
    manifest.save().expect("save package.json");
    Project { root_dir: project.root_dir, manifest, dependency_manifest: None }
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
