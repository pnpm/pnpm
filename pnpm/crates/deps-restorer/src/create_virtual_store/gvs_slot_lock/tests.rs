use super::{GlobalVirtualStoreSlot, GlobalVirtualStoreSlotPhase};
use crate::VirtualStoreLayout;
use pnpm_lockfile::SnapshotEntry;
use std::{collections::HashMap, path::PathBuf};
use tempfile::tempdir;

#[test]
fn slot_state_changes_only_through_the_locked_owner() {
    let root = tempdir().expect("temporary slot root");
    let key = "dep@1.0.0".parse().expect("package key");
    let slot =
        GlobalVirtualStoreSlot::acquire_path(root.path().join("slot"), &key).expect("lock slot");
    let completion_marker = slot.package_path().join("package.json");

    assert_eq!(
        slot.state(&completion_marker, super::BuildRequirement::Required)
            .expect("read missing slot")
            .phase,
        GlobalVirtualStoreSlotPhase::Missing,
    );
    std::fs::create_dir_all(slot.package_path()).expect("create package directory");
    std::fs::write(&completion_marker, "{}").expect("write completion marker");
    slot.finish_population(super::BuildRequirement::NotRequired).expect("mark slot populated");
    assert_eq!(
        slot.state(&completion_marker, super::BuildRequirement::NotRequired)
            .expect("read populated slot")
            .phase,
        GlobalVirtualStoreSlotPhase::Populated,
    );
    assert!(slot.is_populated().expect("check populated slot"));
    std::fs::write(slot.package_path().join(crate::NEEDS_BUILD_MARKER), "")
        .expect("write pending marker");
    slot.finish_population(super::BuildRequirement::Required).expect("mark build pending");
    assert_eq!(
        slot.state(&completion_marker, super::BuildRequirement::Required)
            .expect("read pending build")
            .phase,
        GlobalVirtualStoreSlotPhase::BuildPending,
    );
    assert!(!slot.is_populated().expect("check pending slot"));
    slot.mark_build_started().expect("mark build started");
    assert_eq!(
        slot.state(&completion_marker, super::BuildRequirement::Required)
            .expect("read active build")
            .phase,
        GlobalVirtualStoreSlotPhase::Building,
    );
    slot.mark_build_finished().expect("mark build finished");
    assert_eq!(
        slot.state(&completion_marker, super::BuildRequirement::Required)
            .expect("read finished build")
            .phase,
        GlobalVirtualStoreSlotPhase::Built,
    );
    slot.record_linked_archive("sha512-archive").expect("record linked archive");
    assert_eq!(
        slot.state(&completion_marker, super::BuildRequirement::Required)
            .expect("read linked archive")
            .linked_archive
            .as_deref(),
        Some("sha512-archive"),
    );
}

#[test]
fn acquire_ordered_locks_slots_in_path_order_and_deduplicates_keys() {
    let root = tempdir().expect("temporary slot root");
    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.store_dir = root.path().join("store").into();
    config.global_virtual_store_dir = root.path().join("store/links");
    let keys = [
        "z@1.0.0".parse().expect("package key"),
        "a@1.0.0".parse().expect("package key"),
        "z@1.0.0".parse().expect("package key"),
    ];
    let snapshots = keys
        .iter()
        .cloned()
        .map(|key| (key, SnapshotEntry::default()))
        .collect::<HashMap<_, _>>();
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);

    let slots = GlobalVirtualStoreSlot::acquire_ordered(&layout, keys).expect("lock slots");
    let paths = slots
        .iter()
        .map(super::GlobalVirtualStoreSlot::path)
        .collect::<Vec<_>>();

    assert_eq!(paths.len(), 2);
    assert!(
        paths
            .windows(2)
            .all(|pair| pair[0] < pair[1]),
    );
}

#[test]
fn nested_install_recognizes_a_slot_held_by_its_parent_build() {
    let root = tempdir().expect("temporary slot root");
    let slot_path = root.path().join("slot");
    std::fs::create_dir(&slot_path).expect("create slot path");
    #[cfg(unix)]
    let alias = {
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&slot_path, &alias).expect("create slot alias");
        alias
    };
    #[cfg(not(unix))]
    let alias = slot_path.clone();

    assert!(
        super::slot_lock_is_inherited(&alias, std::slice::from_ref(&slot_path))
            .expect("compare slot paths"),
    );
    assert!(
        !super::slot_lock_is_inherited(&alias, &[root.path().join("other")])
            .expect("compare distinct slot paths"),
    );
}

#[test]
fn nested_install_does_not_wait_for_a_different_busy_slot() {
    let root = tempdir().expect("temporary slot root");
    let key = "dep@1.0.0".parse().expect("package key");
    let parent_path = root.path().join("parent-slot");
    let contested_path = root.path().join("contested-slot");
    std::fs::create_dir_all(&parent_path).expect("create parent slot");
    let contested_slot =
        GlobalVirtualStoreSlot::acquire_path(contested_path.clone(), &key).expect("lock slot");

    let result = GlobalVirtualStoreSlot::acquire_path_with_inherited(
        contested_path,
        &key,
        std::slice::from_ref(&parent_path),
    );
    let Err(error) = result else {
        panic!("nested installs must not wait on another install's slot");
    };

    assert_eq!(error.source.kind(), std::io::ErrorKind::WouldBlock);
    drop(contested_slot);
}

#[test]
fn missing_slot_paths_are_not_inherited() {
    let root = tempdir().expect("temporary slot root");
    let missing_slot = root.path().join("pruned-slot");

    assert!(!super::slot_lock_is_inherited(&missing_slot, &[]).expect("empty inherited slots"));
    assert!(
        !super::slot_lock_is_inherited(&missing_slot, std::slice::from_ref(&missing_slot))
            .expect("pruned inherited slot"),
    );
}

#[test]
fn nested_install_slot_paths_round_trip() {
    let slot_paths = [PathBuf::from("/store/links/package/hash")];
    let encoded = super::encode_slot_lock_paths(&slot_paths).expect("encode slot paths");

    assert_eq!(
        super::path_codec::parse_slot_lock_paths(&encoded).expect("decode slot paths"),
        slot_paths,
    );
    assert!(
        super::path_codec::parse_slot_lock_paths("[").is_err(),
        "malformed slot data must fail",
    );
}

#[cfg(unix)]
#[test]
fn nested_install_slot_paths_preserve_non_utf8_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let slot_paths = [PathBuf::from(std::ffi::OsString::from_vec(b"/store/\xff".to_vec()))];
    let encoded = super::encode_slot_lock_paths(&slot_paths).expect("encode slot paths");

    assert_eq!(
        super::path_codec::parse_slot_lock_paths(&encoded).expect("decode slot paths"),
        slot_paths,
    );
}

#[test]
fn existing_slot_acquisition_rejects_the_empty_parent_left_by_prune() {
    let root = tempdir().expect("temporary slot root");
    let slot_path = root.path().join("slot");
    std::fs::create_dir(&slot_path).expect("create the pruned slot path");
    let key = "dep@1.0.0".parse().expect("package key");

    let slot = GlobalVirtualStoreSlot::acquire_existing_path(slot_path.clone(), &key)
        .expect("inspect slot");

    assert!(slot.is_none(), "the newly created lock directory is not a populated slot");
    assert!(!slot_path.exists(), "the empty slot directory is removed");
}

#[test]
fn only_an_unlocked_settled_slot_opens_without_its_lock() {
    let root = tempdir().expect("temporary slot root");
    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.store_dir = root.path().join("store").into();
    config.global_virtual_store_dir = root.path().join("store/links");
    let key: pnpm_lockfile::PackageKey = "dep@1.0.0".parse().expect("package key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(GlobalVirtualStoreSlot::open_settled(&layout, &key).is_none());

    let slot =
        GlobalVirtualStoreSlot::acquire(&layout, &key).expect("lock slot").expect("hashed slot");
    std::fs::create_dir_all(slot.package_path()).expect("create package directory");
    slot.begin_population().expect("mark slot populating");
    assert!(GlobalVirtualStoreSlot::open_settled(&layout, &key).is_none());
    slot.finish_population(super::BuildRequirement::NotRequired).expect("mark slot populated");
    assert!(GlobalVirtualStoreSlot::open_settled(&layout, &key).is_none());

    drop(slot);
    let settled = GlobalVirtualStoreSlot::open_settled(&layout, &key).expect("settled slot");
    assert_eq!(settled.path(), layout.slot_dir(&key));
}
