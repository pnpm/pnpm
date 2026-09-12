use super::{super::integrity_equal, metadata_with_integrity};

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
