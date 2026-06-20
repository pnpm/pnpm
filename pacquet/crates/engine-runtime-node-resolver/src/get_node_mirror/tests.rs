use std::collections::HashMap;

use pretty_assertions::assert_eq;

use super::get_node_mirror;

/// Mirrors upstream's
/// [`getNodeMirror.test.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/test/getNodeMirror.test.ts).
#[test]
fn configured_mirror_per_channel_wins_over_default() {
    for (channel, host) in [
        ("release", "http://test.mirror.localhost/release"),
        ("nightly", "http://test.mirror.localhost/nightly"),
        ("rc", "http://test.mirror.localhost/rc"),
        ("test", "http://test.mirror.localhost/test"),
        ("v8-canary", "http://test.mirror.localhost/v8-canary"),
    ] {
        let mirrors = HashMap::from([(channel.to_string(), host.to_string())]);
        assert_eq!(get_node_mirror(Some(&mirrors), channel), format!("{host}/"));
    }
}

#[test]
fn uses_defaults_when_unconfigured() {
    let empty = HashMap::new();
    assert_eq!(get_node_mirror(Some(&empty), "release"), "https://nodejs.org/download/release/");
    assert_eq!(get_node_mirror(None, "release"), "https://nodejs.org/download/release/");
}

#[test]
fn appends_trailing_slash_when_missing() {
    let mirrors =
        HashMap::from([("release".to_string(), "http://test.mirror.localhost".to_string())]);
    assert_eq!(get_node_mirror(Some(&mirrors), "release"), "http://test.mirror.localhost/");
}
