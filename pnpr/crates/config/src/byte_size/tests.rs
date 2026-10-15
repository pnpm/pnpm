use super::{ByteSize, parse_byte_size};

#[test]
fn parse_byte_size_handles_integers_and_units() {
    assert_eq!(parse_byte_size("0"), Some(0));
    assert_eq!(parse_byte_size("1024"), Some(1024));
    assert_eq!(parse_byte_size("1024B"), Some(1024));
    assert_eq!(parse_byte_size("1024 bytes"), Some(1024));
    assert_eq!(parse_byte_size("10K"), Some(10 * 1024));
    assert_eq!(parse_byte_size("10KB"), Some(10 * 1024));
    assert_eq!(parse_byte_size("10KiB"), Some(10 * 1024));
    assert_eq!(parse_byte_size("500M"), Some(500 * 1024 * 1024));
    assert_eq!(parse_byte_size("500MB"), Some(500 * 1024 * 1024));
    assert_eq!(parse_byte_size("500MiB"), Some(500 * 1024 * 1024));
    assert_eq!(parse_byte_size("10G"), Some(10 * 1024 * 1024 * 1024));
    assert_eq!(parse_byte_size("10GB"), Some(10 * 1024 * 1024 * 1024));
    assert_eq!(parse_byte_size("10GiB"), Some(10 * 1024 * 1024 * 1024));
    assert_eq!(parse_byte_size("2TB"), Some(2 * 1024 * 1024 * 1024 * 1024));
}

#[test]
fn parse_byte_size_rejects_invalid_values() {
    assert_eq!(parse_byte_size(""), None);
    assert_eq!(parse_byte_size("   "), None);
    assert_eq!(parse_byte_size("-10MB"), None);
    assert_eq!(parse_byte_size("invalid"), None);
    assert_eq!(parse_byte_size("100xyz"), None);
}

#[test]
fn byte_size_deserializes_from_yaml_types() {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        size: ByteSize,
    }

    let from_int: Wrapper = serde_saphyr::from_str("size: 1048576").unwrap();
    assert_eq!(from_int.size, ByteSize("1048576".to_string()));

    let from_str: Wrapper = serde_saphyr::from_str("size: 10GB").unwrap();
    assert_eq!(from_str.size, ByteSize("10GB".to_string()));
}
