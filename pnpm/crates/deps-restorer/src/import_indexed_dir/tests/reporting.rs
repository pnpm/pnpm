use super::super::claim_dir;
use tempfile::tempdir;

// Both stacks let the exclusive mkdir, not the earlier stat, decide who imports a shared slot.
#[test]
fn claim_dir_reports_only_the_creating_call() {
    let tmp = tempdir().unwrap();
    let slot = tmp.path().join("nested").join("slot");

    assert!(claim_dir(&slot).unwrap(), "the call that creates the slot owns it");
    assert!(!claim_dir(&slot).unwrap(), "a later call finds it taken");
}
