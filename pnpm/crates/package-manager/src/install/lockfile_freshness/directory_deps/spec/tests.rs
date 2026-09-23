use super::{is_workspace_path, spec_satisfies_snapshot_dep};
use std::path::PathBuf;

#[test]
fn home_relative_workspace_spec_satisfies_link_target() {
    let home = home::home_dir().unwrap_or_default();
    let workspace_root = PathBuf::from("/workspace");
    let lockfile_dir = workspace_root.clone();
    let local_dep_dir = workspace_root.join("packages/foo");
    let home_pkg = home.join("pkg");
    let rel_to_lockfile = pathdiff::diff_paths(&home_pkg, &lockfile_dir)
        .unwrap_or(home_pkg)
        .display()
        .to_string()
        .replace('\\', "/");
    let lockfile_dep = pnpm_lockfile::SnapshotDepRef::Link(rel_to_lockfile);

    assert!(spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:~/pkg",
        &lockfile_dep,
    ));
    assert!(spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        r"workspace:~\pkg",
        &lockfile_dep,
    ));
}

#[test]
fn recognizes_windows_and_unc_workspace_paths() {
    assert!(is_workspace_path(r"\\server\share\@scope\pkg"));
    assert!(is_workspace_path(r"\root\@scope\pkg"));
    assert!(is_workspace_path(r"~\@scope\pkg"));
    assert!(is_workspace_path("~/pkg"));
    assert!(is_workspace_path(r"c:\@scope\pkg"));
    assert!(!is_workspace_path("@scope/pkg@^1.0.0"));
}

#[test]
fn workspace_range_rejects_registry_snapshot_and_accepts_file_snapshot() {
    let workspace_root = PathBuf::from("/workspace");
    let lockfile_dir = workspace_root.clone();
    let local_dep_dir = workspace_root.join("packages/foo");

    let registry_dep: pnpm_lockfile::SnapshotDepRef = "1.0.0".parse().unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &registry_dep,
    ));

    let file_dep: pnpm_lockfile::SnapshotDepRef = "file:packages/pkg".parse().unwrap();
    assert!(spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &file_dep,
    ));
}
