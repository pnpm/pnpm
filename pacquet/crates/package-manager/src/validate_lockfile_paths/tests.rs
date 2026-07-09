use super::validate_virtual_store_slot_containment;
use crate::VirtualStoreLayout;
use miette::Diagnostic;
use pacquet_lockfile::{PackageKey, SnapshotEntry};
use std::{collections::HashMap, path::PathBuf};

fn assert_invalid_dependency_name_code(err: &pacquet_lockfile_verification::VerifyError) {
    let code = err.code().map(|code| code.to_string());
    assert_eq!(code.as_deref(), Some("ERR_PNPM_INVALID_DEPENDENCY_NAME"));
}

#[test]
fn accepts_snapshots_whose_slots_stay_in_the_store() {
    let layout = VirtualStoreLayout::legacy(
        PathBuf::from("/project/node_modules/.pnpm"),
        pacquet_config::default_virtual_store_dir_max_length() as usize,
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

    let mut config = pacquet_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None);
    assert!(
        !pacquet_fs::is_subdir(layout.package_store_dir(), &layout.slot_dir(&key)),
        "the crafted slot must actually escape the store for this test to be meaningful",
    );

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a slot that escapes the store root must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("evil@"), "offender listed: {err}");
}
