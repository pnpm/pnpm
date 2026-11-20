use super::{
    super::cache_keys::SlotReuse,
    metadata_with_integrity,
    name,
};
use pnpm_lockfile::{
    PackageKey,
    PackageMetadata,
    PkgVerPeer,
};
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

#[test]
fn force_replaces_the_slot_with_or_without_previous_records() {
    let wanted = packages(OLD);
    let current = packages(OLD);
    for current_packages in [None, Some(&current)] {
        let reuse = SlotReuse { packages: &wanted, current_packages, force: true };
        assert!(reuse.must_replace(&key()), "force must replace even unchanged packages");
    }
}

/// A first install has no records either, but its slots do not exist yet,
/// so replacing them is the import's business rather than this decision's.
#[test]
fn a_first_install_without_force_leaves_the_verdict_to_the_import() {
    let wanted = packages(NEW);
    let reuse = SlotReuse { packages: &wanted, current_packages: None, force: false };
    assert!(!reuse.must_replace(&key()));
}
