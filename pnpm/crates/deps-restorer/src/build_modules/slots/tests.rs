use super::built_slot_is_complete;
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
