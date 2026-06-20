use pretty_assertions::assert_eq;

use super::{ParseNodeSpecifierError, parse_node_specifier};

/// Mirrors upstream's
/// [`parseNodeSpecifier.test.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/test/parseNodeSpecifier.test.ts).
#[test]
fn matches_upstream_table() {
    let cases: &[(&str, &str, &str)] = &[
        ("6", "6", "release"),
        ("16.0", "16.0", "release"),
        ("16.0.0-rc.0", "16.0.0-rc.0", "rc"),
        ("rc/10", "10", "rc"),
        ("nightly", "latest", "nightly"),
        ("rc", "latest", "rc"),
        ("test", "latest", "test"),
        ("v8-canary", "latest", "v8-canary"),
        ("release", "latest", "release"),
        ("lts", "lts", "release"),
        ("latest", "latest", "release"),
        ("argon", "argon", "release"),
        ("iron", "iron", "release"),
        ("22.0.0", "22.0.0", "release"),
        ("release/22.0.0", "22.0.0", "release"),
        ("release/latest", "latest", "release"),
        ("release/lts", "lts", "release"),
        ("release/18", "18", "release"),
        ("rc/18", "18", "rc"),
        ("rc/18.0.0-rc.4", "18.0.0-rc.4", "rc"),
        ("nightly/latest", "latest", "nightly"),
        ("24.0.0-nightly20250315d765e70802", "24.0.0-nightly20250315d765e70802", "nightly"),
        ("22.0.0-v8-canary20250101abc", "22.0.0-v8-canary20250101abc", "v8-canary"),
    ];
    for (input, expected_version, expected_channel) in cases {
        let parsed = parse_node_specifier(input)
            .unwrap_or_else(|err| panic!("parse_node_specifier({input}) returned an error: {err}"));
        assert_eq!(parsed.version_specifier, *expected_version, "specifier `{input}`");
        assert_eq!(parsed.release_channel, *expected_channel, "specifier `{input}`");
    }
}

#[test]
fn unknown_channel_raises() {
    match parse_node_specifier("foo/18").unwrap_err() {
        ParseNodeSpecifierError::InvalidReleaseChannel { channel } => assert_eq!(channel, "foo"),
    }
}
