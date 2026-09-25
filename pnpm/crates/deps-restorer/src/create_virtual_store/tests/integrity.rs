use super::{super::integrity_equal, metadata_with_integrity};
use pnpm_lockfile::PackageMetadata;

#[test]
fn integrity_equal_matches_when_integrities_agree() {
    let entry_a = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let entry_b = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    assert!(integrity_equal(Some(&entry_a), Some(&entry_b)));
}
#[test]
fn integrity_equal_distinguishes_changed_integrities() {
    let entry_a = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let entry_b = metadata_with_integrity(
        "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
    );
    assert!(!integrity_equal(Some(&entry_a), Some(&entry_b)));
}
/// Missing metadata on either side (a malformed lockfile, or the
/// snapshot referring to a `packages:` entry that was dropped)
/// collapses to `None` on the integrity lookup. Both sides `None`
/// stays "equal" so a directory/git resolution pair (whose integrity
/// is `None`) doesn't trip a spurious re-fetch.
#[test]
fn integrity_equal_treats_none_pair_as_equal() {
    assert!(integrity_equal(None, None));
}
#[test]
fn integrity_equal_treats_one_sided_missing_as_unequal() {
    let with_integrity = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    assert!(!integrity_equal(None, Some(&with_integrity)));
    assert!(!integrity_equal(Some(&with_integrity), None));
}
#[test]
fn integrity_equal_compares_custom_resolution_integrities() {
    let custom = |integrity: &str| -> PackageMetadata {
        serde_json::from_value(serde_json::json!({
            "resolution": { "type": "custom:served", "integrity": integrity },
        }))
        .expect("parse custom package metadata")
    };

    assert!(integrity_equal(Some(&custom("sha512-old")), Some(&custom("sha512-old"))));
    assert!(!integrity_equal(Some(&custom("sha512-old")), Some(&custom("sha512-new"))));

    let without_integrity: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": { "type": "custom:served" },
    }))
    .expect("parse custom package metadata");
    assert!(!integrity_equal(Some(&custom("sha512-old")), Some(&without_integrity)));
    assert!(integrity_equal(Some(&without_integrity), Some(&without_integrity)));
}
