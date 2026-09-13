use super::is_valid_old_npm_package_name;

#[test]
fn is_valid_old_npm_package_name_accepts_common_shapes() {
    for ok in ["foo", "foo-bar", "foo.bar", "foo_bar", "@scope/foo", "Foo", "1.2.3"] {
        assert!(is_valid_old_npm_package_name(ok), "{ok} should be valid");
    }
}

#[test]
fn is_valid_old_npm_package_name_rejects_error_cases() {
    for bad in [
        "",                 // empty
        ".foo",             // leading dot
        "_foo",             // leading underscore
        "-foo",             // leading hyphen
        " foo",             // leading whitespace
        "foo ",             // trailing whitespace
        "node_modules",     // exclusion list
        "Node_Modules",     // exclusion list, case-insensitive
        "favicon.ico",      // exclusion list
        "foo bar",          // space inside (not URL-safe)
        "foo/bar",          // unscoped slash
        "@scope/.foo",      // scoped, but bare half starts with `.`
        "pnpm:foo",         // colon (not URL-safe, not a scoped shape)
        "^1.2.3",           // caret (not URL-safe)
        "@scope/foo/extra", // scoped shape with extra slash
        "@/foo",            // scoped shape with empty user
        "@scope/",          // scoped shape with empty pkg
    ] {
        assert!(!is_valid_old_npm_package_name(bad), "{bad:?} should be invalid");
    }
}
