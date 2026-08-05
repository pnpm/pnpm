use super::{create_hash, create_hash_from_file, create_short_hash, shorten_virtual_store_name};

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
