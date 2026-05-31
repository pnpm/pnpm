use super::try_get_package_id;

/// Mirrors pnpm's `tryGetPackageId` test in
/// [`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L110-L115).
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

/// A bare `name@version` has no suffix and no `:`, so it round-trips
/// verbatim.
#[test]
fn bare_name_version_round_trips() {
    assert_eq!(try_get_package_id("foo@1.0.0"), "foo@1.0.0");
    assert_eq!(try_get_package_id("@foo/bar@1.0.0"), "@foo/bar@1.0.0");
}

/// A URL-shaped resolution id (`<name>@<url>`) drops the name
/// prefix — that's the whole reason the `:` branch exists. The
/// transform unconditionally applies once a `:` is present and
/// the body isn't `runtime:`.
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

/// `runtime:` entries keep the `<name>@runtime:<version>` form —
/// pnpm's `tryGetPackageId` carves this out via the
/// `!newPkgId.startsWith('runtime:')` guard.
#[test]
fn runtime_entries_keep_name_prefix() {
    assert_eq!(try_get_package_id("node@runtime:22.0.0"), "node@runtime:22.0.0");
    assert_eq!(try_get_package_id("node@runtime:22.0.0(some@peer)"), "node@runtime:22.0.0");
}
