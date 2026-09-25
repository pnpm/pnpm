use super::{
    create_hash, create_hash_from_file, create_hex_hash, create_hex_hash_bytes,
    create_hex_hash_from_file, create_short_hash, integrity_addressed_tarball_integrity,
    integrity_addressed_tarball_path, shorten_virtual_store_name,
};
use ssri::Integrity;

/// Pinned vector against the shell oracle:
///
/// ```sh
/// printf pacquet | openssl dgst -sha256 -binary | base64
/// # => Z4Te8BkaDdaBA6BatwCzHAp8RNp/i/+GfuqATZ6KrPA=
/// ```
#[test]
fn hash_is_sha256_base64_with_prefix() {
    assert_eq!(create_hash("pacquet"), "sha256-Z4Te8BkaDdaBA6BatwCzHAp8RNp/i/+GfuqATZ6KrPA=");
    assert_ne!(create_hash("pacquet"), create_hash("pacquet "));
}

#[test]
fn hash_from_file_normalizes_crlf() {
    let dir = tempfile::TempDir::new().unwrap();
    let crlf = dir.path().join("crlf.txt");
    let lf = dir.path().join("lf.txt");
    std::fs::write(&crlf, "a\r\nb\r\n").unwrap();
    std::fs::write(&lf, "a\nb\n").unwrap();
    assert_eq!(create_hash_from_file(&crlf).unwrap(), create_hash_from_file(&lf).unwrap());
    assert_eq!(create_hash_from_file(&lf).unwrap(), create_hash("a\nb\n"));
}

#[test]
fn hex_hash_accepts_arbitrary_bytes() {
    assert_eq!(create_hex_hash_bytes(b"pacquet"), create_hex_hash("pacquet"));
    assert_ne!(create_hex_hash_bytes(&[0xff]), create_hex_hash_bytes(&[0xfe]));

    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"pacquet").unwrap();
    assert_eq!(create_hex_hash_from_file(file.path()).unwrap(), create_hex_hash("pacquet"));
}

/// Pinned vector against the shell oracle:
///
/// ```sh
/// printf pacquet | shasum -a 256 | head -c 32
/// # => 6784def0191a0dd68103a05ab700b31c
/// ```
#[test]
fn short_hash_is_first_32_hex_chars_of_sha256() {
    let got = create_short_hash("pacquet");
    assert_eq!(got, "6784def0191a0dd68103a05ab700b31c");
    assert_eq!(got.len(), 32);
    assert_ne!(got, create_short_hash("pacquet "));
}

#[test]
fn shorten_below_threshold_is_identity() {
    let name = "ts-node@10.9.1_@types+node@18.7.19_typescript@5.1.6".to_string();
    assert!(name.len() < 120);
    assert_eq!(shorten_virtual_store_name(name.clone(), 120), name);
}

#[test]
fn shorten_above_threshold_hashes_to_max_length() {
    let input = "a".repeat(200);
    let shortened = shorten_virtual_store_name(input, 120);
    assert_eq!(shortened.len(), 120);
    let (prefix, hash) = shortened.rsplit_once('_').expect("hash suffix");
    assert_eq!(prefix.len(), 120 - 33);
    assert_eq!(hash.len(), 32);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn shorten_triggered_by_uppercase_unless_file_protocol() {
    let with_caps = "MyPkg@1.0.0".to_string();
    let shortened = shorten_virtual_store_name(with_caps.clone(), 120);
    assert_ne!(shortened, with_caps);
    assert!(shortened.len() <= 120);

    let file_proto = "file+path+with+Caps".to_string();
    assert_eq!(shorten_virtual_store_name(file_proto.clone(), 120), file_proto);
}

#[test]
fn integrity_address_requires_one_complete_canonical_sha512_hash() {
    let digest = format!("{}==", "A".repeat(86));
    let integrity: Integrity = format!("sha512-{digest}").parse().unwrap();
    assert_eq!(
        integrity_addressed_tarball_path(&integrity),
        Some(format!("-/tarballs/sha512/{}", "A".repeat(86))),
    );

    for malformed in [
        "sha512-AAAA",
        &format!("sha512-{}", "A".repeat(1024 * 1024)),
        &format!("sha512-{}", "A".repeat(86)),
        &format!("sha256-{}=", "A".repeat(43)),
        &format!("sha512-{digest} sha512-{digest}"),
    ] {
        let integrity: Integrity = malformed.parse().unwrap();
        assert_eq!(integrity_addressed_tarball_path(&integrity), None, "{malformed}");
    }
}

#[test]
fn integrity_address_digest_round_trips_to_canonical_sha512() {
    let digest = "A".repeat(86);
    let integrity = integrity_addressed_tarball_integrity(&digest).unwrap();
    assert_eq!(
        integrity_addressed_tarball_path(&integrity),
        Some(format!("-/tarballs/sha512/{digest}")),
    );

    for malformed in [
        "A".repeat(85),
        "A".repeat(87),
        format!("{}=", "A".repeat(85)),
        format!("{}+", "A".repeat(85)),
        format!("{}!", "A".repeat(85)),
    ] {
        assert_eq!(integrity_addressed_tarball_integrity(&malformed), None, "{malformed}");
    }
}
