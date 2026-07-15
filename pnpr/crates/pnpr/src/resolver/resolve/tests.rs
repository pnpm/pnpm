use super::{importer_manifest_name, sanitized_importer_dir};
use crate::resolver::protocol::ResolveRequest;
use pacquet_config::Config;
use pacquet_lockfile::{ImporterDepVersion, Lockfile, PkgName};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_store_dir::StoreDir;
use std::sync::Arc;

#[test]
fn root_and_trailing_slashes_normalize_to_dot() {
    assert_eq!(sanitized_importer_dir(".").unwrap(), ".");
    assert_eq!(sanitized_importer_dir("").unwrap(), ".");
    assert_eq!(sanitized_importer_dir("packages/foo/").unwrap(), "packages/foo");
}

#[test]
fn nested_member_dirs_pass_through() {
    assert_eq!(sanitized_importer_dir("project-a").unwrap(), "project-a");
    assert_eq!(sanitized_importer_dir("packages/foo").unwrap(), "packages/foo");
}

#[test]
fn traversal_absolute_and_backslash_dirs_are_rejected() {
    // `/` and `////` are slashes-only: they must be rejected, not trimmed
    // down to the root importer.
    for unsafe_dir in
        ["../escape", "packages/../../etc", "/abs/path", r"packages\foo", "a//b", "/", "////"]
    {
        assert!(
            sanitized_importer_dir(unsafe_dir).is_err(),
            "expected {unsafe_dir:?} to be rejected",
        );
    }
}

#[test]
fn manifest_names_are_distinct_per_dir() {
    assert_eq!(importer_manifest_name("."), "pnpr-resolve");
    assert_ne!(importer_manifest_name("packages/foo"), importer_manifest_name("packages/bar"));
    // `/` → `-` alone would collide these two; escaping `-` first keeps
    // the mapping injective.
    assert_ne!(importer_manifest_name("packages/foo"), importer_manifest_name("packages-foo"));
}

#[tokio::test]
async fn workspace_star_uses_forwarded_project_name() {
    let lockfile = Box::pin(resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": "packages/app",
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:*" }
            },
            {
                "dir": "packages/lib",
                "name": "lib",
                "version": "1.2.3"
            }
        ]
    })))
    .await;

    assert_workspace_link(&lockfile, "packages/app", "lib", "../lib");
}

#[tokio::test]
async fn workspace_range_uses_forwarded_project_version() {
    let lockfile = Box::pin(resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": "packages/app",
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:^1.0.0" }
            },
            {
                "dir": "packages/lib",
                "name": "lib",
                "version": "1.2.3"
            }
        ]
    })))
    .await;

    assert_workspace_link(&lockfile, "packages/app", "lib", "../lib");
}

#[tokio::test]
async fn workspace_name_without_version_uses_default_version() {
    let lockfile = Box::pin(resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": "packages/app",
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:^0.0.0" }
            },
            {
                "dir": "packages/lib",
                "name": "lib"
            }
        ]
    })))
    .await;

    assert_workspace_link(&lockfile, "packages/app", "lib", "../lib");
}

#[tokio::test]
async fn workspace_version_without_name_uses_synthetic_name() {
    let lockfile = Box::pin(resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": ".",
                "dependencies": {
                    "pnpr-importer-packages-lib": "workspace:^1.0.0"
                }
            },
            {
                "dir": "packages/lib",
                "version": "1.2.3"
            }
        ]
    })))
    .await;

    assert_workspace_link(&lockfile, ".", "pnpr-importer-packages-lib", "packages/lib");
}

#[tokio::test]
async fn legacy_projects_without_identity_use_synthetic_name_and_version() {
    let lockfile = Box::pin(resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": ".",
                "dependencies": {
                    "pnpr-importer-packages-lib": "workspace:^0.0.0"
                }
            },
            {
                "dir": "packages/lib"
            }
        ]
    })))
    .await;

    assert_workspace_link(&lockfile, ".", "pnpr-importer-packages-lib", "packages/lib");
}

async fn resolve_json(request: serde_json::Value) -> Lockfile {
    let temp = tempfile::tempdir().expect("create resolver test directory");
    let mut config = Config::new();
    config.offline = true;
    config.enable_global_virtual_store = false;
    config.store_dir = StoreDir::new(temp.path().join("store"));
    config.cache_dir = temp.path().join("cache");
    config.modules_dir = temp.path().join("node_modules");
    config.virtual_store_dir = temp.path().join("node_modules/.pnpm");
    let config = Box::leak(Box::new(config));
    let request: ResolveRequest = serde_json::from_value(request).expect("resolve request parses");

    super::resolve(
        config,
        &Arc::new(ThrottledClient::new_for_installs()),
        &request,
        &Arc::new(AuthHeaders::default()),
        None,
    )
    .await
    .expect("offline workspace resolution succeeds")
}

fn assert_workspace_link(lockfile: &Lockfile, importer: &str, alias: &str, expected_target: &str) {
    let dependencies = lockfile
        .importers
        .get(importer)
        .expect("importer exists")
        .dependencies
        .as_ref()
        .expect("importer dependencies exist");
    let alias = PkgName::parse(alias).expect("dependency alias parses");
    let dependency = dependencies.get(&alias).expect("workspace dependency exists");
    match &dependency.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, expected_target),
        version => panic!("expected workspace link, got {version:?}"),
    }
}
