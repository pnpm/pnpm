use std::collections::HashMap;

use pretty_assertions::assert_eq;

use super::get_node_mirror;

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
        assert_eq!(get_node_mirror(None, Some(&mirrors), channel), format!("{host}/"));
    }
}

#[test]
fn uses_defaults_when_unconfigured() {
    let empty = HashMap::new();
    assert_eq!(
        get_node_mirror(None, Some(&empty), "release"),
        "https://nodejs.org/download/release/",
    );
    assert_eq!(get_node_mirror(None, None, "release"), "https://nodejs.org/download/release/");
}

#[test]
fn appends_trailing_slash_when_missing() {
    let mirrors =
        HashMap::from([("release".to_string(), "http://test.mirror.localhost".to_string())]);
    assert_eq!(get_node_mirror(None, Some(&mirrors), "release"), "http://test.mirror.localhost/");
}

/// `tools.node.mirror` names where every channel comes from, so the
/// channel goes below it the way nodejs.org lays its own tree out.
#[test]
fn the_tool_mirror_is_the_base_every_channel_hangs_off() {
    for channel in ["release", "nightly", "v8-canary"] {
        assert_eq!(
            get_node_mirror(Some("http://test.mirror.localhost/download"), None, channel),
            format!("http://test.mirror.localhost/download/{channel}/"),
        );
    }
    assert_eq!(
        get_node_mirror(Some("http://test.mirror.localhost/download/"), None, "release"),
        "http://test.mirror.localhost/download/release/",
    );
}

/// `node-mirror:<channel>` names one channel where the base names them
/// all, so it decides the channel it names and leaves the rest to the
/// base.
#[test]
fn a_channel_named_outright_wins_over_the_base() {
    let mirrors = HashMap::from([("nightly".to_string(), "http://nightly.localhost".to_string())]);
    assert_eq!(
        get_node_mirror(Some("http://base.localhost"), Some(&mirrors), "nightly"),
        "http://nightly.localhost/",
    );
    assert_eq!(
        get_node_mirror(Some("http://base.localhost"), Some(&mirrors), "release"),
        "http://base.localhost/release/",
    );
}
