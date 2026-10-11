use super::{ProjectManifestsByDir, SpecDirs, is_workspace_path, spec_satisfies_snapshot_dep};
use std::path::{Path, PathBuf};

/// No project manifest is loaded, so every target is read from disk.
fn dirs<'a>(workspace_root: &'a Path, lockfile_dir: &'a Path) -> SpecDirs<'a> {
    static NO_MANIFESTS: std::sync::LazyLock<ProjectManifestsByDir<'static>> =
        std::sync::LazyLock::new(ProjectManifestsByDir::new);
    SpecDirs { workspace_root, lockfile_dir, manifests_by_dir: &NO_MANIFESTS }
}

#[test]
fn home_relative_workspace_spec_satisfies_link_target() {
    let home = pnpm_config::home_dir().unwrap_or_default();
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
        &dirs(&workspace_root, &lockfile_dir),
        &local_dep_dir,
        "pkg",
        "workspace:~/pkg",
        &lockfile_dep,
    ));
    assert!(spec_satisfies_snapshot_dep(
        &dirs(&workspace_root, &lockfile_dir),
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
        &dirs(&workspace_root, &lockfile_dir),
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
        &dirs(&workspace_root, &lockfile_dir),
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &file_dep,
    ));

    std::fs::write(valid_pkg_dir.join("package.json"), r#"{"name":"other","version":"1.2.3"}"#)
        .unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &dirs(&workspace_root, &lockfile_dir),
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &file_dep,
    ));

    std::fs::write(valid_pkg_dir.join("package.json"), r#"{"name":"pkg","version":"2.0.0"}"#)
        .unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &dirs(&workspace_root, &lockfile_dir),
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
        &dirs(&workspace_root, &lockfile_dir),
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
        &dirs(&workspace_root, &lockfile_dir),
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
        &dirs(&workspace_root, &lockfile_dir),
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
        &dirs(&workspace_root, &lockfile_dir),
        &local_dep_dir,
        "pkg",
        "workspace:^1.0.0",
        &bom_dep,
    ));
}

#[test]
fn workspace_range_reads_a_loaded_manifest_before_the_disk() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace_root = fixture.path().to_path_buf();
    let target_dir = workspace_root.join("packages/pkg");
    std::fs::create_dir_all(&target_dir).unwrap();
    let manifest = pnpm_package_manifest::PackageManifest::from_value(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "pkg", "version": "1.2.3" }),
    );
    let manifests_by_dir = ProjectManifestsByDir::from([(target_dir, &manifest)]);
    let dirs = SpecDirs {
        workspace_root: &workspace_root,
        lockfile_dir: &workspace_root,
        manifests_by_dir: &manifests_by_dir,
    };
    let local_dep_dir = workspace_root.join("packages/consumer");

    for (spec, lockfile_dep) in
        [("workspace:*", "file:packages/pkg"), ("workspace:^1.0.0", "link:packages/pkg")]
    {
        let lockfile_dep: pnpm_lockfile::SnapshotDepRef = lockfile_dep.parse().unwrap();
        assert!(
            spec_satisfies_snapshot_dep(&dirs, &local_dep_dir, "pkg", spec, &lockfile_dep),
            "{spec} against {lockfile_dep:?} with no package.json on disk",
        );
    }
    let file_dep: pnpm_lockfile::SnapshotDepRef = "file:packages/pkg".parse().unwrap();
    assert!(!spec_satisfies_snapshot_dep(
        &dirs,
        &local_dep_dir,
        "pkg",
        "workspace:^2.0.0",
        &file_dep
    ));
}

fn loaded_link_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let fixture = tempfile::tempdir().unwrap();
    let workspace_root = fixture.path().join("workspace");
    let outside = fixture.path().join("outside");
    std::fs::create_dir_all(workspace_root.join("packages")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("package.json"), r#"{"name":"pkg","version":"1.2.3"}"#).unwrap();
    (fixture, workspace_root, outside)
}

fn loaded_target_satisfies(workspace_root: &Path, target_dir: PathBuf) -> bool {
    let manifest = pnpm_package_manifest::PackageManifest::from_value(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "pkg", "version": "1.2.3" }),
    );
    let manifests_by_dir = ProjectManifestsByDir::from([(target_dir, &manifest)]);
    let dirs = SpecDirs {
        workspace_root,
        lockfile_dir: workspace_root,
        manifests_by_dir: &manifests_by_dir,
    };
    let file_dep: pnpm_lockfile::SnapshotDepRef = "file:packages/pkg".parse().unwrap();
    spec_satisfies_snapshot_dep(
        &dirs,
        &workspace_root.join("packages/consumer"),
        "pkg",
        "workspace:*",
        &file_dep,
    )
}

#[test]
fn a_loaded_project_linking_outside_the_workspace_is_not_trusted() {
    let (_fixture, workspace_root, outside) = loaded_link_fixture();
    let target_dir = workspace_root.join("packages/pkg");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &target_dir).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, &target_dir).unwrap();
    assert!(!loaded_target_satisfies(&workspace_root, target_dir));
}

#[test]
fn a_loaded_project_whose_package_json_links_outside_the_workspace_is_not_trusted() {
    let (_fixture, workspace_root, outside) = loaded_link_fixture();
    let target_dir = workspace_root.join("packages/pkg");
    std::fs::create_dir_all(&target_dir).unwrap();
    assert!(loaded_target_satisfies(&workspace_root, target_dir.clone()), "no package.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("package.json"), target_dir.join("package.json"))
        .unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(
        outside.join("package.json"),
        target_dir.join("package.json"),
    )
    .unwrap();
    assert!(!loaded_target_satisfies(&workspace_root, target_dir));
}
