use super::{super::cache_keys::SlotReuse, metadata_with_integrity, name};
use pnpm_lockfile::{PackageKey, PackageMetadata, PkgVerPeer};
use std::collections::HashMap;

const OLD: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const NEW: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

fn key() -> PackageKey {
    PackageKey {
        name: name("dep-a"),
        suffix: "1.0.0".parse::<PkgVerPeer>().expect("parse version"),
    }
}

fn packages(integrity: &str) -> HashMap<PackageKey, PackageMetadata> {
    HashMap::from([(key(), metadata_with_integrity(integrity))])
}

#[test]
fn an_unchanged_integrity_keeps_the_slot() {
    let wanted = packages(OLD);
    let current = packages(OLD);
    let reuse = SlotReuse { packages: &wanted, current_packages: Some(&current), force: false };
    assert!(!reuse.must_replace(&key()));
}

#[test]
fn a_changed_integrity_replaces_the_slot() {
    let wanted = packages(NEW);
    let current = packages(OLD);
    let reuse = SlotReuse { packages: &wanted, current_packages: Some(&current), force: false };
    assert!(reuse.must_replace(&key()));
}

/// The state `--force` arrives in: its records are withheld, so the
/// comparison has nothing to compare and the flag decides alone.
#[test]
fn force_replaces_the_slot_with_no_records_to_compare() {
    let wanted = packages(NEW);
    let reuse = SlotReuse { packages: &wanted, current_packages: None, force: true };
    assert!(reuse.must_replace(&key()));
}

/// A first install has no records either, but its slots do not exist yet,
/// so replacing them is the import's business rather than this decision's.
#[test]
fn a_first_install_without_force_leaves_the_verdict_to_the_import() {
    let wanted = packages(NEW);
    let reuse = SlotReuse { packages: &wanted, current_packages: None, force: false };
    assert!(!reuse.must_replace(&key()));
}
