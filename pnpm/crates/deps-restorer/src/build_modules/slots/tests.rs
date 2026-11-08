use super::{built_slot_is_complete, is_started_build_marker, record_started_build};
use std::fs;
use tempfile::tempdir;

#[test]
fn gvs_state_overrides_a_legacy_build_marker() {
    let root = tempdir().expect("temporary slot root");
    let package = root.path().join("node_modules/package");
    fs::create_dir_all(&package).expect("create package directory");
    fs::write(package.join("package.json"), "{}").expect("write package manifest");
    fs::write(package.join(crate::NEEDS_BUILD_MARKER), "started")
        .expect("write legacy build marker");
    fs::write(root.path().join(crate::GVS_SLOT_STATE_FILE), "built").expect("write slot state");

    assert!(built_slot_is_complete(&package).expect("read built slot"));

    fs::write(root.path().join(crate::GVS_SLOT_STATE_FILE), "build-pending")
        .expect("mark slot build-pending");
    assert!(!built_slot_is_complete(&package).expect("read build-pending slot"));

    fs::remove_file(package.join(crate::NEEDS_BUILD_MARKER)).expect("remove build marker");
    fs::write(root.path().join(crate::GVS_SLOT_STATE_FILE), "populated")
        .expect("mark promoted slot populated");
    assert!(built_slot_is_complete(&package).expect("read promoted slot"));
}

#[test]
fn a_build_without_a_slot_lock_records_that_it_started() {
    let root = tempdir().expect("temporary slot root");
    let key: pnpm_lockfile::PackageKey = "dep@1.0.0".parse().expect("package key");
    let marker = root.path().join(crate::NEEDS_BUILD_MARKER);

    record_started_build(&marker, &key);
    assert!(is_started_build_marker(&marker).expect("read build marker"));

    let removed = root
        .path()
        .join("removed")
        .join(crate::NEEDS_BUILD_MARKER);
    record_started_build(&removed, &key);
    assert!(!removed.exists(), "a missing package directory is left alone");
}
