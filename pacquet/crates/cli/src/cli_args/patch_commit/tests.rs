use super::{
    cleanup_after_diff, normalize_patches_dir_name, patch_target_from_state,
    path_from_forward_slash, remove_dir_if_exists,
};
use pacquet_lockfile::{ComVer, Lockfile, LockfileVersion, PackageKey, PackageMetadata};
use pacquet_package_manager::PkgFilesForDiff;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{collections::HashMap, path::PathBuf};
use tempfile::tempdir;

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 }).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    }
}

fn lockfile_with_packages(keys: &[&str]) -> Lockfile {
    let packages =
        keys.iter().map(|key| (key.parse::<PackageKey>().unwrap(), registry_metadata())).collect();
    Lockfile { packages: Some(packages), ..empty_lockfile() }
}

fn registry_metadata() -> PackageMetadata {
    serde_json::from_value(json!({
        "resolution": {
            "integrity": "sha512-aGVsbG8=",
        },
    }))
    .unwrap()
}

#[test]
fn patch_target_from_state_falls_back_to_manifest_name_and_version() {
    let lockfile = lockfile_with_packages(&["foo@1.0.0"]);

    let target = patch_target_from_state("old-name@9.9.9", "foo", "1.0.0", true, &lockfile)
        .expect("target from fallback");

    assert_eq!(target.alias, "foo");
    assert_eq!(target.version, "1.0.0");
    assert_eq!(target.bare_specifier, "1.0.0");
    assert!(target.apply_to_all);
}

#[test]
fn patch_target_from_state_reports_missing_manifest_version() {
    let lockfile = lockfile_with_packages(&["foo@1.0.0"]);

    let err = patch_target_from_state("old-name@9.9.9", "foo", "2.0.0", false, &lockfile)
        .expect_err("missing fallback version");

    assert!(err.to_string().contains("foo@2.0.0"), "error names fallback target: {err}");
}

#[test]
fn patch_target_from_state_reports_installed_name_version_mismatch() {
    let lockfile = lockfile_with_packages(&["foo@1.0.0"]);

    let err = patch_target_from_state("foo", "foo", "2.0.0", false, &lockfile)
        .expect_err("missing selected version");

    assert!(err.to_string().contains("did you forget to install foo@2.0.0"), "{err}");
}

#[test]
fn normalize_patches_dir_name_removes_dot_and_parent_components() {
    assert_eq!(normalize_patches_dir_name("./patches"), "patches");
    assert_eq!(normalize_patches_dir_name("patches/nested/../final"), "patches/final");
    assert_eq!(normalize_patches_dir_name("../outside"), "outside");
    assert_eq!(normalize_patches_dir_name("."), ".");
}

#[cfg(unix)]
#[test]
fn normalize_patches_dir_name_ignores_root_component() {
    assert_eq!(normalize_patches_dir_name("/tmp/patches"), "tmp/patches");
}

#[test]
fn remove_dir_if_exists_removes_existing_dir() {
    let tmp = tempdir().expect("temp dir");
    let dir = tmp.path().join("patch-commit-temp");
    std::fs::create_dir(&dir).expect("create temp dir");

    remove_dir_if_exists(&dir).expect("remove temp dir");

    assert!(!dir.exists(), "temp dir should be removed");
}

#[test]
fn remove_dir_if_exists_reports_non_directory_cleanup_error() {
    let tmp = tempdir().expect("temp dir");
    let path = tmp.path().join("patch-commit-temp");
    std::fs::write(&path, "").expect("create file");

    let err = remove_dir_if_exists(&path).expect_err("file cleanup should fail");

    assert!(path.is_file(), "cleanup errors are ignored but not fatal");
    assert_eq!(err.kind(), std::io::ErrorKind::NotADirectory);
}

#[test]
fn cleanup_after_diff_removes_temporary_filtered_dir() {
    let tmp = tempdir().expect("temp dir");
    let clean_dir = tmp.path().join("clean");
    let filtered_dir = tmp.path().join("filtered");
    std::fs::create_dir(&clean_dir).expect("create clean dir");
    std::fs::create_dir(&filtered_dir).expect("create filtered dir");

    cleanup_after_diff(&clean_dir, &PkgFilesForDiff::Temporary(filtered_dir.clone()))
        .expect("cleanup temp dirs");

    assert!(!clean_dir.exists(), "clean dir should be removed");
    assert!(!filtered_dir.exists(), "filtered dir should be removed");
}

#[test]
fn cleanup_after_diff_keeps_original_patch_dir() {
    let tmp = tempdir().expect("temp dir");
    let clean_dir = tmp.path().join("clean");
    let patch_dir = tmp.path().join("patch");
    std::fs::create_dir(&clean_dir).expect("create clean dir");
    std::fs::create_dir(&patch_dir).expect("create patch dir");

    cleanup_after_diff(&clean_dir, &PkgFilesForDiff::Original(patch_dir.clone()))
        .expect("cleanup clean dir");

    assert!(!clean_dir.exists(), "clean dir should be removed");
    assert!(patch_dir.exists(), "original patch dir should remain");
}

#[test]
fn path_from_forward_slash_builds_platform_path() {
    assert_eq!(
        path_from_forward_slash("patches/@scope__pkg.patch"),
        PathBuf::from("patches").join("@scope__pkg.patch"),
    );
}
