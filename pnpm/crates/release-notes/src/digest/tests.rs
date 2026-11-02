use super::note_digest;

#[test]
fn digests_the_empty_note_like_the_rfc_example() {
    assert_eq!(note_digest(""), "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
}
