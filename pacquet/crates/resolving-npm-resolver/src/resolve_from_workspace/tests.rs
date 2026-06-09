//! Unit tests for [`try_resolve_from_workspace`]. The upstream tests
//! exercise this via the full `resolveNpm` flow; pacquet's tests
//! invoke the helper directly with a hand-built
//! [`WorkspacePackages`] map.

use std::{collections::BTreeMap, path::Path};

use pacquet_lockfile::LockfileResolution;
use pacquet_resolving_resolver_base::{
    WantedDependency, WorkspacePackage, WorkspacePackages, WorkspacePackagesByVersion,
};
use serde_json::json;

use super::{ResolveFromWorkspaceError, ResolveFromWorkspaceOptions, try_resolve_from_workspace};

fn build_packages() -> WorkspacePackages {
    let mut foo: WorkspacePackagesByVersion = BTreeMap::new();
    foo.insert(
        "1.0.0".to_string(),
        WorkspacePackage {
            root_dir: Path::new("/repo/packages/foo").to_path_buf(),
            manifest: json!({ "name": "foo", "version": "1.0.0" }),
        },
    );
    foo.insert(
        "2.0.0".to_string(),
        WorkspacePackage {
            root_dir: Path::new("/repo/packages/foo-2").to_path_buf(),
            manifest: json!({ "name": "foo", "version": "2.0.0" }),
        },
    );

    let mut bar: WorkspacePackagesByVersion = BTreeMap::new();
    bar.insert(
        "0.1.2".to_string(),
        WorkspacePackage {
            root_dir: Path::new("/repo/packages/bar").to_path_buf(),
            manifest: json!({ "name": "bar", "version": "0.1.2" }),
        },
    );

    let mut packages: WorkspacePackages = BTreeMap::new();
    packages.insert("foo".to_string(), foo);
    packages.insert("bar".to_string(), bar);
    packages
}

fn opts(packages: &WorkspacePackages) -> ResolveFromWorkspaceOptions<'_> {
    ResolveFromWorkspaceOptions {
        project_dir: Path::new("/repo/packages/consumer"),
        lockfile_dir: Path::new("/repo"),
        registry: "https://registry.npmjs.org/",
        default_tag: "latest",
        workspace_packages: Some(packages),
        inject_workspace_packages: false,
    }
}

fn wanted(alias: &str, bare: &str) -> WantedDependency {
    WantedDependency {
        alias: Some(alias.to_string()),
        bare_specifier: Some(bare.to_string()),
        ..WantedDependency::default()
    }
}

#[test]
fn non_workspace_spec_returns_none() {
    let packages = build_packages();
    let opts = opts(&packages);
    let result = try_resolve_from_workspace(&wanted("foo", "^1.0.0"), &opts).expect("ok").is_none();
    assert!(result);
}

#[test]
fn workspace_path_form_defers_to_local_resolver() {
    let packages = build_packages();
    let opts = opts(&packages);
    let result = try_resolve_from_workspace(&wanted("foo", "workspace:../foo"), &opts).expect("ok");
    assert!(result.is_none());
}

#[test]
fn workspace_star_resolves_to_link_against_highest_version() {
    let packages = build_packages();
    let opts = opts(&packages);
    let result = try_resolve_from_workspace(&wanted("foo", "workspace:*"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:../foo-2");
    assert_eq!(result.resolved_via, "workspace");
    match &result.resolution {
        LockfileResolution::Directory(dir) => assert_eq!(dir.directory, "../foo-2"),
        other => panic!("expected directory resolution, got {other:?}"),
    }
    assert_eq!(result.alias.as_deref(), Some("foo"));
}

/// A workspace package depending on itself (`project_dir` == the resolved
/// package's `root_dir`) renders as a bare `link:` — the relative path is
/// empty, matching pnpm's `link:${path.relative(projectDir, projectDir)}`
/// (`''`), not `link:.`.
#[test]
fn workspace_self_dependency_renders_as_bare_link() {
    let mut versions: WorkspacePackagesByVersion = BTreeMap::new();
    versions.insert(
        "1.0.0".to_string(),
        WorkspacePackage {
            root_dir: Path::new("/repo/packages/self").to_path_buf(),
            manifest: json!({ "name": "self", "version": "1.0.0" }),
        },
    );
    let mut packages: WorkspacePackages = BTreeMap::new();
    packages.insert("self".to_string(), versions);

    let opts = ResolveFromWorkspaceOptions {
        project_dir: Path::new("/repo/packages/self"),
        lockfile_dir: Path::new("/repo"),
        registry: "https://registry.npmjs.org/",
        default_tag: "latest",
        workspace_packages: Some(&packages),
        inject_workspace_packages: false,
    };
    let result = try_resolve_from_workspace(&wanted("self", "workspace:*"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:");
}

#[test]
fn workspace_caret_range_picks_lower_when_pinned_range_excludes_higher() {
    let packages = build_packages();
    let opts = opts(&packages);
    let result = try_resolve_from_workspace(&wanted("foo", "workspace:^1.0.0"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:../foo");
}

#[test]
fn workspace_exact_version_picks_that_entry() {
    let packages = build_packages();
    let opts = opts(&packages);
    let result = try_resolve_from_workspace(&wanted("bar", "workspace:0.1.2"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:../bar");
}

#[test]
fn aliased_workspace_form_routes_through_package_name() {
    let packages = build_packages();
    let opts = opts(&packages);
    let result = try_resolve_from_workspace(&wanted("bar", "workspace:foo@^1.0.0"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:../foo");
    assert_eq!(result.alias.as_deref(), Some("bar"));
}

#[test]
fn missing_workspace_package_surfaces_pnpm_error_code() {
    let packages = build_packages();
    let opts = opts(&packages);
    let err = try_resolve_from_workspace(&wanted("missing", "workspace:*"), &opts).unwrap_err();
    assert!(matches!(
        err,
        ResolveFromWorkspaceError::WorkspacePkgNotFound { ref name, .. } if name == "missing",
    ));
}

#[test]
fn no_matching_version_surfaces_pnpm_error_code() {
    let packages = build_packages();
    let opts = opts(&packages);
    let err = try_resolve_from_workspace(&wanted("foo", "workspace:^99.0.0"), &opts).unwrap_err();
    assert!(matches!(err, ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace { .. }));
}

#[test]
fn workspace_packages_unset_surfaces_error() {
    let packages = build_packages();
    let mut opts = opts(&packages);
    opts.workspace_packages = None;
    let err = try_resolve_from_workspace(&wanted("foo", "workspace:*"), &opts).unwrap_err();
    assert!(matches!(err, ResolveFromWorkspaceError::WorkspacePackagesNotLoaded));
}

#[test]
fn inject_workspace_packages_writes_file_resolution() {
    let packages = build_packages();
    let mut opts = opts(&packages);
    opts.inject_workspace_packages = true;
    let result = try_resolve_from_workspace(&wanted("foo", "workspace:*"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "file:packages/foo-2");
    match &result.resolution {
        LockfileResolution::Directory(dir) => assert_eq!(dir.directory, "packages/foo-2"),
        other => panic!("expected directory resolution, got {other:?}"),
    }
}

#[test]
fn publish_config_directory_overrides_root_when_link_directory_is_unset() {
    let mut packages: WorkspacePackages = BTreeMap::new();
    let mut entries: WorkspacePackagesByVersion = BTreeMap::new();
    entries.insert(
        "1.0.0".to_string(),
        WorkspacePackage {
            root_dir: Path::new("/repo/packages/foo").to_path_buf(),
            manifest: json!({
                "name": "foo",
                "version": "1.0.0",
                "publishConfig": { "directory": "dist" },
            }),
        },
    );
    packages.insert("foo".to_string(), entries);

    let opts = ResolveFromWorkspaceOptions {
        project_dir: Path::new("/repo/packages/consumer"),
        lockfile_dir: Path::new("/repo"),
        registry: "https://registry.npmjs.org/",
        default_tag: "latest",
        workspace_packages: Some(&packages),
        inject_workspace_packages: false,
    };

    let result = try_resolve_from_workspace(&wanted("foo", "workspace:*"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:../foo/dist");
}

#[test]
fn publish_config_link_directory_false_keeps_root() {
    let mut packages: WorkspacePackages = BTreeMap::new();
    let mut entries: WorkspacePackagesByVersion = BTreeMap::new();
    entries.insert(
        "1.0.0".to_string(),
        WorkspacePackage {
            root_dir: Path::new("/repo/packages/foo").to_path_buf(),
            manifest: json!({
                "name": "foo",
                "version": "1.0.0",
                "publishConfig": { "directory": "dist", "linkDirectory": false },
            }),
        },
    );
    packages.insert("foo".to_string(), entries);

    let opts = ResolveFromWorkspaceOptions {
        project_dir: Path::new("/repo/packages/consumer"),
        lockfile_dir: Path::new("/repo"),
        registry: "https://registry.npmjs.org/",
        default_tag: "latest",
        workspace_packages: Some(&packages),
        inject_workspace_packages: false,
    };

    let result = try_resolve_from_workspace(&wanted("foo", "workspace:*"), &opts)
        .expect("ok")
        .expect("some");
    assert_eq!(result.id.as_str(), "link:../foo");
}
