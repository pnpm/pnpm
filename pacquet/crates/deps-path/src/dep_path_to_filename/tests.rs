use super::dep_path_to_filename;

#[test]
fn plain_name_at_version_round_trips() {
    assert_eq!(dep_path_to_filename("foo@1.0.0", 120), "foo@1.0.0");
}

#[test]
fn scoped_name_keeps_at_replaces_slash_with_plus() {
    assert_eq!(dep_path_to_filename("@scope/foo@1.0.0", 120), "@scope+foo@1.0.0");
}

#[test]
fn peer_suffix_is_flattened_with_underscores() {
    assert_eq!(
        dep_path_to_filename("foo@1.0.0(bar@2.0.0)(baz@3.0.0)", 120),
        "foo@1.0.0_bar@2.0.0_baz@3.0.0",
    );
}

#[test]
fn file_scheme_keeps_path_separators_via_plus_escape() {
    assert_eq!(dep_path_to_filename("file:packages/foo", 120), "file+packages+foo");
}

#[test]
fn exceeding_length_replaces_with_hash_suffix() {
    let very_long_input = format!("foo@1.0.0{}", "(bar@2.0.0)".repeat(40));
    let got = dep_path_to_filename(&very_long_input, 60);
    assert_eq!(got.len(), 60);
    assert!(got.contains('_'));
    let hash_part = &got[got.len() - 32..];
    assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn uppercase_outside_file_scheme_forces_hash_suffix() {
    let got = dep_path_to_filename("FOO@1.0.0", 120);
    assert!(got.starts_with("FOO@1.0.0_"));
    assert_eq!(got.len(), "FOO@1.0.0_".len() + 32);
}

#[test]
fn uppercase_in_file_scheme_is_preserved_untouched() {
    // `file+...` is excluded from the case-mismatch branch — the
    // filesystem casing of `file:` paths is part of the install
    // address, so hashing it would split the cache.
    assert_eq!(dep_path_to_filename("file:Pkg", 120), "file+Pkg");
}

#[test]
fn nested_peer_group_uses_double_underscore_at_boundary() {
    // eslint-plugin-testing-library@7.7.0(eslint@9.35.0(jiti@2.6.1))(typescript@6.0.3)
    // The `))(` sequence should produce `__`: the inner `)` closes the nested
    // group and the outer `)(` separates top-level peers.
    assert_eq!(
        dep_path_to_filename(
            "eslint-plugin-testing-library@7.7.0(eslint@9.35.0(jiti@2.6.1))(typescript@6.0.3)",
            120,
        ),
        "eslint-plugin-testing-library@7.7.0_eslint@9.35.0_jiti@2.6.1__typescript@6.0.3",
    );
}

#[test]
fn empty_input_does_not_panic() {
    assert_eq!(dep_path_to_filename("", 120), "");
}

#[test]
fn single_byte_input_does_not_panic() {
    assert_eq!(dep_path_to_filename("/", 120), "");
    assert_eq!(dep_path_to_filename("a", 120), "a");
}
