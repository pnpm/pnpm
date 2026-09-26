use super::snapshot_is_patched;
use pnpm_lockfile::{PackageKey, SnapshotEntry};

fn key(raw: &str) -> PackageKey {
    raw.parse().expect("parse package key")
}

#[test]
fn own_patch_hash_marks_the_snapshot_patched() {
    assert!(snapshot_is_patched(&key("foo@1.0.0(patch_hash=abc)"), None));
    assert!(snapshot_is_patched(&key("foo@1.0.0(patch_hash=abc)(bar@2.0.0)"), None));
}

#[test]
fn patched_peer_does_not_mark_the_snapshot_patched() {
    assert!(!snapshot_is_patched(&key("foo@1.0.0(bar@2.0.0(patch_hash=abc))"), None));
    assert!(!snapshot_is_patched(&key("foo@1.0.0"), None));
}

#[test]
fn patched_flag_marks_the_snapshot_patched() {
    let snapshot = SnapshotEntry { patched: Some(true), ..SnapshotEntry::default() };
    assert!(snapshot_is_patched(&key("foo@1.0.0"), Some(&snapshot)));
}
