use std::{thread::sleep, time::Duration};

use tempfile::TempDir;

use super::PublicPackages;

fn open() -> (PublicPackages, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let table = PublicPackages::open(&dir.path().join("public.sqlite")).expect("open");
    (table, dir)
}

#[test]
fn records_and_reads_a_classification() {
    let (table, _dir) = open();
    assert!(!table.is_public("lodash", None));
    table.record("lodash");
    assert!(table.is_public("lodash", None));
    // Classification is per name, not per other name.
    assert!(!table.is_public("react", None));
}

#[test]
fn a_ttl_expires_an_old_classification() {
    let (table, _dir) = open();
    table.record("lodash");
    assert!(table.is_public("lodash", Some(Duration::from_mins(1))));
    sleep(Duration::from_millis(5));
    assert!(!table.is_public("lodash", Some(Duration::from_millis(1))));
}

#[test]
fn classifications_persist_across_reopen() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("public.sqlite");
    {
        let table = PublicPackages::open(&path).expect("open");
        table.record("lodash");
    }
    let reopened = PublicPackages::open(&path).expect("reopen");
    assert!(reopened.is_public("lodash", None));
}
