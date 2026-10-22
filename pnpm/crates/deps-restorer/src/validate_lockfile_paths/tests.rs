use super::validate_virtual_store_slot_containment;
use crate::VirtualStoreLayout;
use miette::Diagnostic;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{collections::HashMap, path::PathBuf};

fn assert_invalid_dependency_name_code(err: &pnpm_lockfile_verification::VerifyError) {
    let code = err.code().map(|code| code.to_string());
    assert_eq!(code.as_deref(), Some("ERR_PNPM_INVALID_DEPENDENCY_NAME"));
}

#[test]
fn accepts_snapshots_whose_slots_stay_in_the_store() {
    let layout = VirtualStoreLayout::legacy(
        PathBuf::from("/project/node_modules/.pnpm"),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let mut snapshots = HashMap::new();
    snapshots.insert("@scope/foo@1.2.3".parse::<PackageKey>().unwrap(), SnapshotEntry::default());
    snapshots.insert("bar@4.5.6".parse::<PackageKey>().unwrap(), SnapshotEntry::default());
    validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect("contained slots must pass");
}

#[test]
fn rejects_a_global_virtual_store_version_escape() {
    // Under the global virtual store the slot path inserts the version
    // segment as a raw path component (unlike the legacy flat name, which
    // escapes `/`). A traversal-bearing version escapes the store root
    // even though the package name itself is valid, so the containment
    // check — not the name check — is what rejects it.
    let key: PackageKey = "evil@../../../escaped".parse().expect("parse escaping version key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(
        !pnpm_fs::is_subdir(layout.package_store_dir(), &layout.slot_dir(&key)),
        "the crafted slot must actually escape the store for this test to be meaningful",
    );

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a slot that escapes the store root must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("evil@"), "offender listed: {err}");
}

#[test]
fn rejects_a_global_virtual_store_two_level_version_escape() {
    let key: PackageKey = "evil@../../escaped".parse().expect("parse escaping version key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(!layout.is_slot_contained(&key), "two-level traversal escapes the package directory");

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a slot that escapes the package directory must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("evil@"), "offender listed: {err}");
}

#[test]
fn rejects_a_global_virtual_store_scoped_version_escape() {
    let key: PackageKey = "@scope/evil@../../escaped".parse().expect("parse escaping scoped key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(!layout.is_slot_contained(&key), "scoped traversal escapes the package directory");

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a scoped slot that escapes the package directory must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("@scope/evil@"), "offender listed: {err}");
}

#[test]
fn rejects_a_global_virtual_store_manifest_version_escape() {
    let key: PackageKey = "tar-dep@file:vendor/dep.tgz".parse().expect("parse tarball key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        pnpm_lockfile::PackageMetadata {
            resolution: pnpm_lockfile::LockfileResolution::Tarball(
                pnpm_lockfile::TarballResolution {
                    tarball: "file:vendor/dep.tgz".to_string(),
                    integrity: None,
                    revision: None,
                    git_hosted: None,
                    path: None,
                },
            ),
            version: Some("../../escaped".to_string()),
            engines: None,
            cpu: None,
            os: None,
            libc: None,
            deprecated: None,
            has_bin: None,
            prepare: None,
            bundled_dependencies: None,
            peer_dependencies: None,
            peer_dependencies_meta: None,
        },
    );

    let layout =
        VirtualStoreLayout::new(&config, None, Some(&snapshots), Some(&packages), None, None);
    assert!(
        !layout.is_slot_contained(&key),
        "manifest version traversal escapes the package directory",
    );

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a manifest version traversal slot must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("tar-dep@"), "offender listed: {err}");
}
