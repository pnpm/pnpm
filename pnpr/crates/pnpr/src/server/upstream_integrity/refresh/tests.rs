use super::{BlobEviction, reconcile};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};

#[test]
fn refresh_preserves_matching_pins_and_evicts_removed_or_changed_tarballs() {
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let integrity = ssri::Integrity::from(b"example".as_slice()).to_string();
    let mut old = json!({"versions": {}});
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        old["versions"][version] = json!({"dist": {
            "tarball": format!("https://registry.test/example/-/example-{version}.tgz"),
            "integrity": integrity,
        }});
    }
    let mut new = old.clone();
    new["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    new["versions"]
        .as_object_mut()
        .unwrap()
        .remove("2.0.0");
    new["versions"]["3.0.0"]["dist"]["shasum"] = json!("changed");
    let (bytes, removed) = reconcile(
        Some(&serde_json::to_vec(&old).unwrap()),
        serde_json::to_vec(&new).unwrap(),
        &name,
    )
    .unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["integrity"], integrity);
    let BlobEviction::Files(removed) = removed else {
        panic!("valid metadata permits selective eviction");
    };
    assert_eq!(removed.len(), 2);
    assert!(removed.contains("example-2.0.0.tgz"));
    assert!(removed.contains("example-3.0.0.tgz"));
}

#[test]
fn malformed_cached_metadata_is_replaced_after_purging_its_blobs() {
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let fresh = br#"{"versions":{}}"#.to_vec();
    let (bytes, eviction) = reconcile(Some(b"malformed"), fresh.clone(), &name).unwrap();
    assert_eq!(bytes, fresh);
    assert!(matches!(eviction, BlobEviction::Package));
}
