//! Port of pnpm's
//! [`releasing/exportable-manifest/test/index.test.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/test/index.test.ts)
//! covering the workspace-protocol rewrite. The upstream test goes
//! through `createExportableManifest` (which performs a full install
//! and reads from the resulting `node_modules`); pacquet's test
//! materializes the same `node_modules` layout directly with
//! `tempfile::TempDir` so we exercise `replace_workspace_protocol`
//! and `replace_workspace_protocol_peer_dependency` in isolation.

use std::{fs, path::Path};

use tempfile::TempDir;

use super::{
    CannotResolveWorkspaceProtocolError, ReplaceWorkspaceProtocolError, replace_workspace_protocol,
    replace_workspace_protocol_peer_dependency,
};

/// Materialize the install tree upstream's
/// [`workspace deps are replaced`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/test/index.test.ts#L96-L197)
/// case sets up via `pnpm install`. Returns `(temp_root, project_dir)`:
/// `project_dir = <temp>/workspace-protocol-package` so the relative
/// `workspace:../xerox` resolves to a sibling at `<temp>/xerox`.
fn workspace_fixture() -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().expect("tempdir");
    let project = temp.path().join("workspace-protocol-package");
    let modules = project.join("node_modules");
    fs::create_dir_all(&modules).unwrap();

    // The local alias `bar` resolves to the workspace project named
    // `@foo/bar`; `node_modules/bar/package.json` carries the resolved
    // manifest (same name, copied / linked there by `pnpm install`).
    write_dep(&modules.join("bar"), "@foo/bar", "3.2.1");
    write_dep(&modules.join("baz"), "baz", "1.2.3");
    write_dep(&modules.join("foo"), "foo", "4.5.6");
    write_dep(&modules.join("qux"), "qux", "1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay");
    write_dep(&modules.join("quux"), "quux", "7.8.9");
    write_dep(&modules.join("waldo"), "waldo", "1.9.0");

    // `workspace:../xerox` reads the sibling project directly, not from
    // `node_modules`.
    write_dep(&temp.path().join("xerox"), "xerox", "4.5.6");
    (temp, project)
}

fn write_dep(dir: &Path, name: &str, version: &str) {
    fs::create_dir_all(dir).unwrap();
    let manifest = serde_json::json!({ "name": name, "version": version });
    fs::write(dir.join("package.json"), serde_json::to_string(&manifest).unwrap()).unwrap();
}

fn rewrite(dep_name: &str, dep_spec: &str, dir: &Path) -> String {
    replace_workspace_protocol(dep_name, dep_spec, dir, None).expect("replace succeeds")
}

fn rewrite_peer(dep_name: &str, dep_spec: &str, dir: &Path) -> String {
    replace_workspace_protocol_peer_dependency(dep_name, dep_spec, dir, None)
        .expect("replace succeeds")
}

#[test]
fn passes_through_non_workspace_specs() {
    let dir = TempDir::new().unwrap();
    assert_eq!(rewrite("foo", "^1.0.0", dir.path()), "^1.0.0");
    assert_eq!(rewrite("foo", "npm:bar@1", dir.path()), "npm:bar@1");
}

#[test]
fn workspace_dep_rewrites_match_upstream() {
    let (_fixture, project) = workspace_fixture();
    let dir = project.as_path();

    assert_eq!(rewrite("bar", "workspace:@foo/bar@*", dir), "npm:@foo/bar@3.2.1");
    assert_eq!(rewrite("baz", "workspace:baz@^", dir), "^1.2.3");
    assert_eq!(rewrite("foo", "workspace:*", dir), "4.5.6");
    assert_eq!(
        rewrite("qux", "workspace:^", dir),
        "^1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay",
    );
    assert_eq!(rewrite("quux", "workspace:", dir), "7.8.9");
    assert_eq!(rewrite("waldo", "workspace:^", dir), "^1.9.0");
    assert_eq!(rewrite("xerox", "workspace:../xerox", dir), "4.5.6");
    assert_eq!(rewrite("xeroxAlias", "workspace:../xerox", dir), "npm:xerox@4.5.6");
    assert_eq!(rewrite("corge", "workspace:1.0.0", dir), "1.0.0");
    assert_eq!(rewrite("grault", "workspace:^1.0.0", dir), "^1.0.0");
    assert_eq!(rewrite("garply", "workspace:plugh@2.0.0", dir), "npm:plugh@2.0.0");
}

#[test]
fn peer_workspace_dep_rewrites_match_upstream() {
    let (_fixture, project) = workspace_fixture();
    let dir = project.as_path();

    assert_eq!(rewrite_peer("foo", "workspace:>= || ^3.9.0", dir), ">=4.5.6 || ^3.9.0");
    assert_eq!(rewrite_peer("baz", "^1.0.0 || workspace:>", dir), "^1.0.0 || >1.2.3");
    assert_eq!(rewrite_peer("bar", "workspace:^3.0.0", dir), "^3.0.0");
    assert_eq!(
        rewrite_peer("qux", "workspace:^", dir),
        "^1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay",
    );
    assert_eq!(rewrite_peer("waldo", "workspace:^1.x", dir), "^1.x");
}

/// Upstream uses JS `String.prototype.replace('workspace:', '')`, which
/// strips only the first occurrence.
#[test]
fn peer_workspace_strip_only_removes_first_occurrence() {
    let (_fixture, project) = workspace_fixture();
    let dir = project.as_path();

    assert_eq!(
        rewrite_peer("baz", "workspace:^1.0.0 || workspace:^2.0.0", dir),
        "^1.0.0 || workspace:^2.0.0",
    );
}

#[test]
fn missing_dependency_surfaces_cannot_resolve_error() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    fs::create_dir_all(dir.join("node_modules")).unwrap();

    let err = replace_workspace_protocol("ghost", "workspace:*", dir, None).unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
        }) if dep_name == "ghost"
    ));
}

#[test]
fn missing_dependency_surfaces_cannot_resolve_error_for_peer() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    fs::create_dir_all(dir.join("node_modules")).unwrap();

    let err =
        replace_workspace_protocol_peer_dependency("ghost", "workspace:^", dir, None).unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
        }) if dep_name == "ghost"
    ));
}

/// `workspace:*` doesn't reach into the manifest for the version
/// token (the version comes from the lookup), so when the dep name
/// doesn't match the resolved manifest's name the output is an
/// `npm:`-aliased reference.
#[test]
fn dep_name_mismatch_routes_to_npm_alias() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    let modules = dir.join("node_modules");
    write_dep(&modules.join("local-name"), "actual-name", "1.2.3");

    assert_eq!(rewrite("local-name", "workspace:*", dir), "npm:actual-name@1.2.3");
}
