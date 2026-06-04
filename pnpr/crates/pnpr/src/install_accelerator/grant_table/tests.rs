use std::{thread::sleep, time::Duration};

use tempfile::TempDir;

use super::GrantTable;

fn open() -> (GrantTable, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let table = GrantTable::open(&dir.path().join("grants.sqlite")).expect("open grant table");
    (table, dir)
}

#[test]
fn records_and_reads_a_grant() {
    let (table, _dir) = open();
    assert!(!table.is_granted("alice", "@acme/foo@1.0.0", None));
    table.record("alice", "@acme/foo@1.0.0");
    assert!(table.is_granted("alice", "@acme/foo@1.0.0", None));
    // A grant is per-user and per-version.
    assert!(!table.is_granted("bob", "@acme/foo@1.0.0", None));
    assert!(!table.is_granted("alice", "@acme/foo@2.0.0", None));
}

#[test]
fn clear_package_drops_every_version_for_that_user_only() {
    let (table, _dir) = open();
    table.record("alice", "@acme/foo@1.0.0");
    table.record("alice", "@acme/foo@2.0.0");
    table.record("alice", "@acme/bar@1.0.0");
    table.record("bob", "@acme/foo@1.0.0");

    table.clear_package("alice", "@acme/foo");

    assert!(!table.is_granted("alice", "@acme/foo@1.0.0", None));
    assert!(!table.is_granted("alice", "@acme/foo@2.0.0", None));
    // A different package the same user holds is untouched.
    assert!(table.is_granted("alice", "@acme/bar@1.0.0", None));
    // Another user's grant for the same package is untouched.
    assert!(table.is_granted("bob", "@acme/foo@1.0.0", None));
}

#[test]
fn clear_package_does_not_prefix_match_a_sibling_name() {
    let (table, _dir) = open();
    // `foo` must not clear `foo-bar` — the `@`-delimited prefix guards it.
    table.record("alice", "foo@1.0.0");
    table.record("alice", "foo-bar@1.0.0");
    table.clear_package("alice", "foo");
    assert!(!table.is_granted("alice", "foo@1.0.0", None));
    assert!(table.is_granted("alice", "foo-bar@1.0.0", None));
}

#[test]
fn a_ttl_expires_an_old_grant() {
    let (table, _dir) = open();
    table.record("alice", "foo@1.0.0");
    // Still valid under a generous TTL.
    assert!(table.is_granted("alice", "foo@1.0.0", Some(Duration::from_secs(60))));
    // Expired under a zero TTL once any time has passed.
    sleep(Duration::from_millis(5));
    assert!(!table.is_granted("alice", "foo@1.0.0", Some(Duration::from_millis(1))));
}

#[test]
fn grants_persist_across_reopen() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("grants.sqlite");
    {
        let table = GrantTable::open(&path).expect("open");
        table.record("alice", "foo@1.0.0");
    }
    let reopened = GrantTable::open(&path).expect("reopen");
    assert!(reopened.is_granted("alice", "foo@1.0.0", None));
}
