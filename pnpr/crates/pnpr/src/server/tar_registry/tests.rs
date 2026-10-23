use super::{BTreeMap, DerivedTar, restore_tgz_integrities, tgz_integrity_key};
use serde_json::json;

const TAR_INTEGRITY: &str = "sha512-tar";

fn table_for(stored_dist: &serde_json::Value) -> BTreeMap<String, DerivedTar> {
    let key = tgz_integrity_key(stored_dist.as_object().unwrap()).unwrap();
    BTreeMap::from([(key, DerivedTar::new(TAR_INTEGRITY, None, None, None))])
}

#[test]
fn the_tar_integrity_of_a_hosted_version_becomes_the_stored_integrity() {
    let stored_dist = json!({ "integrity": "sha512-dGd6", "tarball": "http://x/a/-/a-1.0.0.tgz" });
    let hosted = json!({ "versions": { "1.0.0": { "dist": stored_dist } } });
    let mut incoming =
        json!({ "versions": { "1.0.0": { "dist": { "integrity": TAR_INTEGRITY } } } });

    restore_tgz_integrities(&hosted, &table_for(&stored_dist), &mut incoming);

    assert_eq!(incoming["versions"]["1.0.0"]["dist"]["integrity"], "sha512-dGd6");
}

#[test]
fn the_tar_integrity_of_a_shasum_only_version_is_dropped() {
    let stored_dist = json!({ "shasum": "0123456789abcdef0123456789abcdef01234567" });
    let hosted = json!({ "versions": { "1.0.0": { "dist": stored_dist } } });
    let mut incoming =
        json!({ "versions": { "1.0.0": { "dist": { "integrity": TAR_INTEGRITY } } } });

    restore_tgz_integrities(&hosted, &table_for(&stored_dist), &mut incoming);

    assert!(incoming["versions"]["1.0.0"]["dist"].get("integrity").is_none());
}

#[test]
fn an_integrity_that_is_not_the_tar_integrity_is_kept() {
    let stored_dist = json!({ "integrity": "sha512-dGd6" });
    let hosted = json!({ "versions": { "1.0.0": { "dist": stored_dist } } });
    let mut incoming =
        json!({ "versions": { "1.0.0": { "dist": { "integrity": "sha512-other" } } } });

    restore_tgz_integrities(&hosted, &table_for(&stored_dist), &mut incoming);

    assert_eq!(incoming["versions"]["1.0.0"]["dist"]["integrity"], "sha512-other");
}
