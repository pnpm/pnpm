use pretty_assertions::assert_eq;

use super::{ParseNodeSpecifierError, parse_node_specifier};

/// Mirrors upstream's
/// [`parseNodeSpecifier.test.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/test/parseNodeSpecifier.test.ts)
/// table verbatim: every accepted input shape, each paired with the
/// expected `(version_specifier, release_channel)` pair.
#[test]
fn matches_upstream_table() {
    let cases: &[(&str, &str, &str)] = &[
        // Semver ranges → release channel.
        ("6", "6", "release"),
        ("16.0", "16.0", "release"),
        // Exact prerelease with rc channel.
        ("16.0.0-rc.0", "16.0.0-rc.0", "rc"),
        // Channel/range combo (major only).
        ("rc/10", "10", "rc"),
        // Standalone channel name → latest from that channel.
        ("nightly", "latest", "nightly"),
        ("rc", "latest", "rc"),
        ("test", "latest", "test"),
        ("v8-canary", "latest", "v8-canary"),
        ("release", "latest", "release"),
        // Well-known aliases.
        ("lts", "lts", "release"),
        ("latest", "latest", "release"),
        // LTS codenames.
        ("argon", "argon", "release"),
        ("iron", "iron", "release"),
        // Exact stable version.
        ("22.0.0", "22.0.0", "release"),
        // Stable release with explicit channel prefix, aliases, and ranges.
        ("release/22.0.0", "22.0.0", "release"),
        ("release/latest", "latest", "release"),
        ("release/lts", "lts", "release"),
        ("release/18", "18", "release"),
        // Channel/version combos.
        ("rc/18", "18", "rc"),
        ("rc/18.0.0-rc.4", "18.0.0-rc.4", "rc"),
        ("nightly/latest", "latest", "nightly"),
        // Exact nightly version.
        ("24.0.0-nightly20250315d765e70802", "24.0.0-nightly20250315d765e70802", "nightly"),
        // Exact v8-canary version.
        ("22.0.0-v8-canary20250101abc", "22.0.0-v8-canary20250101abc", "v8-canary"),
    ];
    for (input, expected_version, expected_channel) in cases {
        let parsed = parse_node_specifier(input)
            .unwrap_or_else(|err| panic!("parse_node_specifier({input}) returned an error: {err}"));
        assert_eq!(parsed.version_specifier, *expected_version, "specifier `{input}`");
        assert_eq!(parsed.release_channel, *expected_channel, "specifier `{input}`");
    }
}

/// An unknown channel name on the left of `/` raises
/// `INVALID_NODE_RELEASE_CHANNEL`. Mirrors upstream's
/// `throws for unknown release channel` test.
#[test]
fn unknown_channel_raises() {
    match parse_node_specifier("foo/18").unwrap_err() {
        ParseNodeSpecifierError::InvalidReleaseChannel { channel } => assert_eq!(channel, "foo"),
    }
}
