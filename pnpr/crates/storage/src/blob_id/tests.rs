use super::{BlobId, parse_blob_key};
use pnpr_package_name::Ecosystem;

#[test]
fn an_identity_is_the_ecosystems_digest_in_hex() {
    let npm = BlobId::of_bytes(Ecosystem::Npm, b"bytes");
    assert!(npm.as_str().starts_with("sha512-"), "{npm}");
    assert_eq!(npm.as_str().len(), "sha512-".len() + 128);
    let oci = BlobId::of_bytes(Ecosystem::Oci, b"bytes");
    assert_eq!(oci.as_str(), format!("sha256-{}", pnpr_oci_hex(b"bytes")));
    let integrity = ssri::Integrity::from(b"bytes");
    assert_eq!(BlobId::from_integrity(&integrity, Ecosystem::Pypi), Some(oci));
    assert_eq!(BlobId::from_integrity(&integrity, Ecosystem::Npm), None);
}

#[test]
fn a_key_names_one_incarnation() {
    let id = BlobId::of_bytes(Ecosystem::Cargo, b"crate");
    assert_eq!(parse_blob_key(&id.key(7)), Some((id.clone(), 7)));
    for name in [
        id.as_str().to_string(),
        format!("{id}.0"),
        format!("{id}.07"),
        format!("{id}.x"),
        format!("{id}."),
        "pkg-1.0.0.tgz".to_string(),
        format!("{}.1", id.as_str().to_uppercase()),
    ] {
        assert_eq!(parse_blob_key(&name), None, "{name}");
    }
}

/// The hex SHA-256 of `bytes`.
fn pnpr_oci_hex(bytes: &[u8]) -> String {
    let (_, hex) = ssri::Integrity::from(bytes).to_hex();
    hex
}
