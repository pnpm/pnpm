use super::{Add, normalized_save_specifier};
use crate::ResolvedPackages;
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::PinnedVersion;
use pacquet_reporter::SilentReporter;
use std::sync::Arc;
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
        .mock("GET", "/@private/foo/latest")
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
        .mock("GET", "/@private/foo/latest")
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
        list_dependency_groups: || [DependencyGroup::Prod],
        package_name: "@private/foo",
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

fn scoped_version_body(registry_url: &str) -> String {
    format!(
        r#"{{
  "name": "@private/foo",
  "version": "1.0.0",
  "dist": {{
    "integrity": "{SCOPED_TEST_INTEGRITY}",
    "tarball": "{registry_url}@private/foo/-/foo-1.0.0.tgz"
  }}
}}"#,
    )
}

fn scoped_package_body(registry_url: &str) -> String {
    format!(
        r#"{{
  "name": "@private/foo",
  "dist-tags": {{ "latest": "1.0.0" }},
  "versions": {{
    "1.0.0": {{
      "name": "@private/foo",
      "version": "1.0.0",
      "dist": {{
        "integrity": "{SCOPED_TEST_INTEGRITY}",
        "tarball": "{registry_url}@private/foo/-/foo-1.0.0.tgz"
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
