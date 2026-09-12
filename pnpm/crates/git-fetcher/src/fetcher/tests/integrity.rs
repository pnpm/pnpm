use super::is_valid_commit_hash;

#[test]
fn is_valid_commit_hash_accepts_full_sha() {
    assert!(is_valid_commit_hash("c9b30e71d704cd30fa71f2edd1ecc7dcc4985493"));
    assert!(is_valid_commit_hash("C9B30E71D704CD30FA71F2EDD1ECC7DCC4985493"));
}

#[test]
fn is_valid_commit_hash_rejects_short_or_option_shaped_values() {
    assert!(!is_valid_commit_hash("deadbeef"));
    assert!(!is_valid_commit_hash(""));
    assert!(!is_valid_commit_hash("--upload-pack=touch /tmp/pwned"));
    assert!(!is_valid_commit_hash("c9b30e71d704cd30fa71f2edd1ecc7dcc4985493 "));
    // 40 chars but contains a non-hex digit.
    assert!(!is_valid_commit_hash("c9b30e71d704cd30fa71f2edd1ecc7dcc498549z"));
}
