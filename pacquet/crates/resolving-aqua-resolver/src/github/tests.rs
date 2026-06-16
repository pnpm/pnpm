use super::parse_checksum_file;

#[test]
fn parses_hash_and_filename_rows() {
    let map = parse_checksum_file(
        "abc123  ripgrep-14.1.1-x86_64-apple-darwin.tar.gz\ndef456  ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz\n",
    );
    assert_eq!(map.get("ripgrep-14.1.1-x86_64-apple-darwin.tar.gz"), Some(&"abc123".to_string()));
    assert_eq!(
        map.get("ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"),
        Some(&"def456".to_string()),
    );
}

#[test]
fn stores_single_column_hash_under_the_empty_key() {
    let map = parse_checksum_file("deadbeef\n");
    assert_eq!(map.get(""), Some(&"deadbeef".to_string()));
}

#[test]
fn skips_blank_lines() {
    let map = parse_checksum_file("\n\nabc  file.tar.gz\n\n");
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("file.tar.gz"), Some(&"abc".to_string()));
}
