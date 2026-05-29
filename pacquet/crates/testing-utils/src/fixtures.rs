pub const BIG_MANIFEST: &str = include_str!("fixtures/big/package.json");
// perfectionist's comment-scanning rules textually scan `include_str!`-ed
// files; the `//` inside a `https://` URL in this lockfile's `deprecated`
// field is misread as a line comment, so `bare_url` flags it and its autofix
// would rewrite the fixture. Suppress at the include site until the upstream
// bug (KSXGitHub/perfectionist) is fixed.
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::bare_url,
        reason = "false positive on a URL inside include_str!-ed YAML data; rewriting the fixture would corrupt it"
    )
)]
pub const BIG_LOCKFILE: &str = include_str!("fixtures/big/pnpm-lock.yaml");
