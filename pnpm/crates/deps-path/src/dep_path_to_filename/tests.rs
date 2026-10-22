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

#[test]
fn trailing_dots_and_spaces_are_escaped() {
    assert_eq!(
        dep_path_to_filename("parent-pkg@file:..", 120),
        "parent-pkg@file+++_3cf6176c884f1541b42906b711973e2d",
    );
    assert_eq!(
        dep_path_to_filename("pkg@file:.", 120),
        "pkg@file++_f8a4bd4027dd0dda71549ddff4eb2bbb",
    );
    assert_eq!(
        dep_path_to_filename("pkg@file:../dir ", 120),
        "pkg@file+..+dir+_58ccd8dce4811ac686d72920d5724090",
    );
    assert_eq!(
        dep_path_to_filename("foo@1.0.0(pkg@file:..)", 120),
        "foo@1.0.0_pkg@file+++_532b5e0801878347427004a15da818ef",
    );
    assert_eq!(dep_path_to_filename("pkg@file:../project-2", 120), "pkg@file+..+project-2");
}

#[test]
fn escaped_trailing_dots_do_not_collide_with_literal_plus() {
    assert_ne!(
        dep_path_to_filename("parent-pkg@file:..", 120),
        dep_path_to_filename("parent-pkg@file:++", 120),
    );
    assert_eq!(
        dep_path_to_filename("parent-pkg@file:++", 120),
        "parent-pkg@file+++_533e2d775a8ebec1166dcc5f3df4de30",
    );
    assert_ne!(
        dep_path_to_filename("Parent-pkg@file:..", 120),
        dep_path_to_filename("Parent-pkg@file:++", 120),
    );
}

#[test]
fn urls_with_ambiguous_escapes_are_hashed() {
    let base = "pkg@https://registry.example.com/objects/trusted";
    let cases = [
        ("/package.tgz", "pkg@https+++registry.example.com+objects+trusted+package.tgz"),
        (
            "+package.tgz",
            "pkg@https+++registry.example.com+objects+trusted+package.tgz_5b1382866f68e516badc06db3d605b81",
        ),
        (
            ":package.tgz",
            "pkg@https+++registry.example.com+objects+trusted+package.tgz_1536617e57923d7a634ea5063d52d423",
        ),
        (
            "?package.tgz",
            "pkg@https+++registry.example.com+objects+trusted+package.tgz_56ed6c679663e41c52a329ea7ffc783f",
        ),
        (
            "#package.tgz",
            "pkg@https+++registry.example.com+objects+trusted+package.tgz_30421c24d7ece624b64805feea2d87f4",
        ),
        (
            r"\package.tgz",
            "pkg@https+++registry.example.com+objects+trusted+package.tgz_c28afc38506f4f342864ce00ee3373c8",
        ),
    ];
    for (suffix, expected) in cases {
        assert_eq!(dep_path_to_filename(&format!("{base}{suffix}"), 120), expected);
    }
    assert_eq!(
        dep_path_to_filename(&format!("{base}+package.tgz"), 40),
        "pkg@htt_5b1382866f68e516badc06db3d605b81",
    );
    assert_eq!(
        dep_path_to_filename(&format!("{base}/package.tgz"), 40),
        "pkg@htt_db0178b93a3dc73ecc84ba68ab60a4ab",
    );
}

#[test]
fn git_urls_are_hashed_only_with_an_ambiguous_escape() {
    assert_eq!(
        dep_path_to_filename("foo@git+https://github.com/something/foo", 120),
        "foo@git+https+++github.com+something+foo",
    );
    assert_eq!(
        dep_path_to_filename("foo@git+https://github.com/something/foo#1234", 120),
        "foo@git+https+++github.com+something+foo+1234_b1a78add6ab51a177ff8780d0f64b41d",
    );
    assert_eq!(
        dep_path_to_filename(
            "foo@https://codeload.github.com/something/foo/tar.gz/1234#path:packages/foo",
            120,
        ),
        "foo@https+++codeload.github.com+something+foo+tar.gz+1234+path+packages+foo_160b2e15ba002509e0f467796aa2dfd1",
    );
}

#[test]
fn registry_versions_with_build_metadata_are_not_hashed() {
    assert_eq!(dep_path_to_filename("foo@1.0.0+build.5", 120), "foo@1.0.0+build.5");
    assert_eq!(
        dep_path_to_filename("esbuild@0.0.0-dev+abc(foo@1.0.0)", 120),
        "esbuild@0.0.0-dev+abc_foo@1.0.0",
    );
    assert_eq!(dep_path_to_filename("foo@work:1.0.0+build", 120), "foo@work+1.0.0+build");
}
