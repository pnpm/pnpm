use super::{GlobalVirtualStoreSlot, GlobalVirtualStoreSlotPhase};
use tempfile::tempdir;

#[test]
fn slot_state_changes_only_through_the_locked_owner() {
    let root = tempdir().expect("temporary slot root");
    let key = "dep@1.0.0".parse().expect("package key");
    let slot =
        GlobalVirtualStoreSlot::acquire_path(root.path().join("slot"), &key).expect("lock slot");
    let completion_marker = slot.package_path().join("package.json");

    assert_eq!(
        slot.state(&completion_marker, true).expect("read missing slot").phase,
        GlobalVirtualStoreSlotPhase::Missing,
    );
    std::fs::create_dir_all(slot.package_path()).expect("create package directory");
    std::fs::write(&completion_marker, "{}").expect("write completion marker");
    slot.finish_population(false).expect("mark slot populated");
    assert_eq!(
        slot.state(&completion_marker, false).expect("read populated slot").phase,
        GlobalVirtualStoreSlotPhase::Populated,
    );
    std::fs::write(slot.package_path().join(crate::NEEDS_BUILD_MARKER), "")
        .expect("write pending marker");
    slot.finish_population(true).expect("mark build pending");
    assert_eq!(
        slot.state(&completion_marker, true).expect("read pending build").phase,
        GlobalVirtualStoreSlotPhase::BuildPending,
    );
    slot.mark_build_started().expect("mark build started");
    assert_eq!(
        slot.state(&completion_marker, true).expect("read active build").phase,
        GlobalVirtualStoreSlotPhase::Building,
    );
    slot.mark_build_finished().expect("mark build finished");
    assert_eq!(
        slot.state(&completion_marker, true).expect("read finished build").phase,
        GlobalVirtualStoreSlotPhase::Built,
    );
    slot.record_linked_archive("sha512-archive").expect("record linked archive");
    assert_eq!(
        slot.state(&completion_marker, true)
            .expect("read linked archive")
            .linked_archive
            .as_deref(),
        Some("sha512-archive"),
    );
}
