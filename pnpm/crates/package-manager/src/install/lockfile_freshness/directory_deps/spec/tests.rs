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
fn workspace_range_validates_file_snapshot_target() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
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

    let valid_pkg_dir = workspace_root.join("packages/pkg");
    std::fs::create_dir_all(&valid_pkg_dir).unwrap();
    std::fs::write(valid_pkg_dir.join("package.json"), r#"{"name":"pkg","version":"1.2.3"}"#)
        .unwrap();

    let file_dep: pnpm_lockfile::SnapshotDepRef = "file:packages/pkg".parse().unwrap();
    assert!(spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &file_dep,
    ));

    std::fs::write(valid_pkg_dir.join("package.json"), r#"{"name":"other","version":"1.2.3"}"#)
        .unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &file_dep,
    ));

    std::fs::write(valid_pkg_dir.join("package.json"), r#"{"name":"pkg","version":"2.0.0"}"#)
        .unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &file_dep,
    ));

    let outside_temp = tempfile::tempdir().unwrap();
    let outside_pkg_dir = outside_temp.path().join("pkg");
    std::fs::create_dir_all(&outside_pkg_dir).unwrap();
    std::fs::write(outside_pkg_dir.join("package.json"), r#"{"name":"pkg","version":"1.2.3"}"#)
        .unwrap();

    let outside_dep: pnpm_lockfile::SnapshotDepRef = "file:../../outside/pkg".parse().unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &outside_dep,
    ));

    let symlink_pkg_dir = workspace_root.join("packages/symlink-pkg");
    std::fs::create_dir_all(&symlink_pkg_dir).unwrap();
    std::fs::write(valid_pkg_dir.join("package.json"), r#"{"name":"pkg","version":"1.2.3"}"#)
        .unwrap();
    let symlink_manifest = symlink_pkg_dir.join("package.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(valid_pkg_dir.join("package.json"), &symlink_manifest).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(valid_pkg_dir.join("package.json"), &symlink_manifest)
        .unwrap();
    let symlink_dep: pnpm_lockfile::SnapshotDepRef = "file:packages/symlink-pkg".parse().unwrap();
    assert!(spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &symlink_dep,
    ));

    std::fs::remove_file(&symlink_manifest).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside_pkg_dir.join("package.json"), &symlink_manifest).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(outside_pkg_dir.join("package.json"), &symlink_manifest)
        .unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &symlink_dep,
    ));
}

#[test]
fn manifest_with_utf8_bom_satisfies_workspace_spec() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace_root = fixture.path().to_path_buf();
    let lockfile_dir = workspace_root.clone();
    let local_dep_dir = workspace_root.join("packages/consumer");
    let bom_pkg_dir = workspace_root.join("packages/bom-pkg");
    std::fs::create_dir_all(&bom_pkg_dir).unwrap();
    std::fs::write(
        bom_pkg_dir.join("package.json"),
        format!("\u{feff}{}", r#"{"name":"pkg","version":"1.2.3"}"#),
    )
    .unwrap();
    let bom_dep: pnpm_lockfile::SnapshotDepRef = "file:packages/bom-pkg".parse().unwrap();
    assert!(spec_satisfies_snapshot_dep(
        &workspace_root,
        &lockfile_dir,
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &bom_dep,
    ));
}
