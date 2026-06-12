use pacquet_lockfile::PkgNameVerPeer;

use super::landed_on_prior_entry;

fn key(raw: &str) -> PkgNameVerPeer {
    raw.parse().expect("parse snapshot key")
}

#[test]
fn matches_a_plain_registry_id() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0"));
    assert!(!landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.1.0"));
}

#[test]
fn strips_the_recorded_key_peer_and_patch_suffixes() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0(bar@2.0.0)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)(bar@2.0.0)"), "foo@1.0.0"));
}

#[test]
fn strips_the_resolved_id_patch_suffix() {
    assert!(landed_on_prior_entry(
        &key("foo@1.0.0(patch_hash=0000)"),
        "foo@1.0.0(patch_hash=0000)"
    ));
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0(patch_hash=0000)"));
}

#[test]
fn matches_a_name_prefixed_file_id() {
    // `build_pkg_id_with_patch_hash` prefixes `file:` / git / tarball
    // ids with the manifest name, matching the recorded key shape.
    assert!(landed_on_prior_entry(&key("foo@file:packages/foo"), "foo@file:packages/foo"));
    assert!(!landed_on_prior_entry(&key("foo@file:packages/foo"), "file:packages/foo"));
}
