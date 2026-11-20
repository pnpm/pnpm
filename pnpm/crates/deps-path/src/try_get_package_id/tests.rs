use super::try_get_package_id;

#[test]
fn matches_pnpm_test_cases() {
    assert_eq!(try_get_package_id("/foo@1.0.0(@types/babel__core@7.1.14)"), "/foo@1.0.0");
    assert_eq!(
        try_get_package_id("/foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))"),
        "/foo@1.0.0",
    );
    assert_eq!(
        try_get_package_id("/@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)"),
        "/@(-.-)/foo@1.0.0",
    );
    assert_eq!(
        try_get_package_id("foo@1.0.0(patch_hash=xxxx)(@types/babel__core@7.1.14)"),
        "foo@1.0.0",
    );
}

#[test]
fn bare_name_version_round_trips() {
    assert_eq!(try_get_package_id("foo@1.0.0"), "foo@1.0.0");
    assert_eq!(try_get_package_id("@foo/bar@1.0.0"), "@foo/bar@1.0.0");
}

#[test]
fn url_shape_drops_name_prefix() {
    assert_eq!(
        try_get_package_id("foo@https://example.com/foo.tgz"),
        "https://example.com/foo.tgz",
    );
    assert_eq!(
        try_get_package_id("foo@git+https://github.com/x/foo#abc"),
        "git+https://github.com/x/foo#abc",
    );
}

#[test]
fn runtime_entries_keep_name_prefix() {
    assert_eq!(try_get_package_id("node@runtime:22.0.0"), "node@runtime:22.0.0");
    assert_eq!(try_get_package_id("node@runtime:22.0.0(some@peer)"), "node@runtime:22.0.0");
    assert_eq!(try_get_package_id("node@tool:22.0.0"), "node@tool:22.0.0");
}

#[test]
fn multibyte_utf8_and_short_inputs_do_not_panic() {
    assert_eq!(
        try_get_package_id("ä@https://example.invalid/a.tgz"),
        "https://example.invalid/a.tgz",
    );
    assert_eq!(
        try_get_package_id("@ä/ü@https://example.invalid/a.tgz"),
        "https://example.invalid/a.tgz",
    );
    assert_eq!(
        try_get_package_id("😀@https://example.invalid/a.tgz(peer@1.0.0)"),
        "https://example.invalid/a.tgz",
    );
    assert_eq!(try_get_package_id("ä@1.0.0"), "ä@1.0.0");
    assert_eq!(try_get_package_id("ä@runtime:22.0.0"), "ä@runtime:22.0.0");
    assert_eq!(try_get_package_id("ä:1.0.0"), "ä:1.0.0");
    assert_eq!(try_get_package_id(":"), ":");
    assert_eq!(try_get_package_id(""), "");
}
