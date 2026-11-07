use super::{GlobalVirtualStoreSlot, GlobalVirtualStoreSlotPhase};
use crate::VirtualStoreLayout;
use pnpm_lockfile::SnapshotEntry;
use std::collections::HashMap;
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
            .all(|pair| pair[0] < pair[1])
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
