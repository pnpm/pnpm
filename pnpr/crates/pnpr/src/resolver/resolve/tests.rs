use super::{importer_manifest_name, sanitized_importer_dir};
use crate::resolver::protocol::ResolveRequest;
use pacquet_config::{Config, LinkWorkspacePackages};
use pacquet_lockfile::{ImporterDepVersion, Lockfile, PkgName};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_store_dir::StoreDir;
use std::sync::Arc;

#[test]
fn exact_dot_is_the_only_root_importer() {
    assert_eq!(sanitized_importer_dir(".").unwrap(), ".");
}

#[test]
fn nested_member_dirs_pass_through() {
    assert_eq!(sanitized_importer_dir("project-a").unwrap(), "project-a");
    assert_eq!(sanitized_importer_dir("packages/foo").unwrap(), "packages/foo");
}

#[test]
fn traversal_absolute_and_backslash_dirs_are_rejected() {
    for unsafe_dir in [
        "../escape",
        "packages/../../etc",
        "/abs/path",
        "//server/share",
        r"packages\foo",
        r"\\server\share",
    ] {
        assert!(
            sanitized_importer_dir(unsafe_dir).is_err(),
            "expected {unsafe_dir:?} to be rejected",
        );
    }
}

#[test]
fn empty_and_dot_components_are_rejected() {
    for unsafe_dir in ["", "/", "////", "a//b", "packages/foo/", "./packages/foo", "packages/./foo"]
    {
        assert!(
            sanitized_importer_dir(unsafe_dir).is_err(),
            "expected {unsafe_dir:?} to be rejected",
        );
    }
}

#[test]
fn windows_drive_and_colon_forms_are_rejected() {
    for unsafe_dir in ["C:/outside", "C:relative", "packages/foo:bar"] {
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
async fn forwarded_catalogs_resolve_catalog_specifiers() {
    // The reconstructed workspace carries no catalog sections, so a
    // `catalog:` specifier resolves only because the request's `catalogs`
    // are forwarded into the install as the catalog set. The catalog entry
    // is a plain version that matches a workspace sibling, so
    // `link-workspace-packages` links it and the assertion stays offline.
    let lockfile = Box::pin(resolve_json_with(
        serde_json::json!({
            "projects": [
                {
                    "dir": "packages/app",
                    "name": "app",
                    "version": "1.0.0",
                    "dependencies": { "lib": "catalog:" }
                },
                {
                    "dir": "packages/lib",
                    "name": "lib",
                    "version": "1.2.3"
                }
            ],
            "catalogs": {
                "default": { "lib": "^1.0.0" }
            }
        }),
        |config| config.link_workspace_packages = LinkWorkspacePackages::Deep,
    ))
    .await;

    assert_workspace_link(&lockfile, "packages/app", "lib", "../lib");
}

#[tokio::test]
async fn workspace_without_root_project_has_no_synthetic_root_importer() {
    let lockfile = Box::pin(resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": "packages/app",
                "name": "app",
                "version": "1.0.0"
            },
            {
                "dir": "packages/lib",
                "name": "lib",
                "version": "1.2.3"
            }
        ]
    })))
    .await;

    assert_eq!(
        lockfile.importers.keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["packages/app", "packages/lib"]),
    );
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

/// `packages/Foo` and `packages/foo` are two directories on a
/// case-sensitive filesystem and one directory on a case-insensitive one.
/// Both outcomes are acceptable; what must never happen is one importer's
/// `package.json` overwriting the other's and both resolving to the same
/// dependency map. The two aliases depend on different workspace siblings,
/// so a collision is visible in the resolved links rather than only in the
/// importer keys, which survive the overwrite either way.
#[tokio::test]
async fn case_aliasing_importer_dirs_never_drop_a_project() {
    let outcome = Box::pin(try_resolve_json(serde_json::json!({
        "projects": [
            {
                "dir": "packages/Foo",
                "name": "foo-upper",
                "version": "1.0.0",
                "dependencies": { "lib-upper": "workspace:*" }
            },
            {
                "dir": "packages/foo",
                "name": "foo-lower",
                "version": "1.0.0",
                "dependencies": { "lib-lower": "workspace:*" }
            },
            { "dir": "packages/lib-upper", "name": "lib-upper", "version": "1.0.0" },
            { "dir": "packages/lib-lower", "name": "lib-lower", "version": "1.0.0" }
        ]
    })))
    .await;

    match outcome {
        Ok(lockfile) => {
            assert_workspace_link(&lockfile, "packages/Foo", "lib-upper", "../lib-upper");
            assert_workspace_link(&lockfile, "packages/foo", "lib-lower", "../lib-lower");
        }
        Err(error) => {
            let error = format!("{error:?}");
            assert!(
                error.contains("duplicate importer dir"),
                "a case-insensitive filesystem must reject the alias, got {error}",
            );
        }
    }
}

async fn resolve_json(request: serde_json::Value) -> Lockfile {
    try_resolve_json(request).await.expect("offline workspace resolution succeeds")
}

async fn resolve_json_with(
    request: serde_json::Value,
    configure: impl FnOnce(&mut Config),
) -> Lockfile {
    try_resolve_json_with(request, configure).await.expect("offline workspace resolution succeeds")
}

async fn try_resolve_json(request: serde_json::Value) -> Result<Lockfile, super::ResolveError> {
    try_resolve_json_with(request, |_| {}).await
}

async fn try_resolve_json_with(
    request: serde_json::Value,
    configure: impl FnOnce(&mut Config),
) -> Result<Lockfile, super::ResolveError> {
    let temp = tempfile::tempdir().expect("create resolver test directory");
    let mut config = Config::new();
    config.offline = true;
    config.enable_global_virtual_store = false;
    config.store_dir = StoreDir::new(temp.path().join("store"));
    config.cache_dir = temp.path().join("cache");
    config.modules_dir = temp.path().join("node_modules");
    config.virtual_store_dir = temp.path().join("node_modules/.pnpm");
    configure(&mut config);
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
