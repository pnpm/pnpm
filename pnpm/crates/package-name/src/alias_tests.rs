use super::is_valid_dependency_alias;

#[test]
fn accepts_valid_aliases() {
    for alias in ["foo", "Foo", "@scope/name", "@s/x", "lodash.merge", "a_b", "a-b", "underscore"] {
        assert!(is_valid_dependency_alias(alias), "expected valid: {alias:?}");
    }
}

#[test]
fn rejects_invalid_aliases() {
    for alias in [
        "",
        "..",
        ".",
        "/foo",
        "foo/bar",
        "@scope/name/extra",
        "@scope/../etc",
        "@x/../../../../../.git/hooks",
        r"foo\bar",
        r"C:\Windows\System32",
        "foo\0bar",
        "scope/name",
        "./foo",
        ".bin",
        ".pnpm",
        "_foo",
        "node_modules",
        "favicon.ico",
        "  foo  ",
        "foo bar",
        "foo?bar",
    ] {
        assert!(!is_valid_dependency_alias(alias), "expected invalid: {alias:?}");
    }
}
