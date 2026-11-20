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
        assert_eq!(get_node_mirror(None, None, Some(&mirrors), channel), format!("{host}/"));
    }
}

#[test]
fn uses_defaults_when_unconfigured() {
    let empty = HashMap::new();
    assert_eq!(
        get_node_mirror(None, None, Some(&empty), "release"),
        "https://nodejs.org/download/release/",
    );
    assert_eq!(
        get_node_mirror(None, None, None, "release"),
        "https://nodejs.org/download/release/",
    );
}

#[test]
fn appends_trailing_slash_when_missing() {
    let mirrors =
        HashMap::from([("release".to_string(), "http://test.mirror.localhost".to_string())]);
    assert_eq!(
        get_node_mirror(None, None, Some(&mirrors), "release"),
        "http://test.mirror.localhost/",
    );
}

/// nodejs.org lays its channels out below one base, which is the shape
/// a mirror of it carries.
#[test]
fn the_tool_mirror_is_the_base_every_channel_hangs_off() {
    for channel in ["release", "nightly", "v8-canary"] {
        assert_eq!(
            get_node_mirror(Some("http://test.mirror.localhost/download"), None, None, channel),
            format!("http://test.mirror.localhost/download/{channel}/"),
        );
    }
    assert_eq!(
        get_node_mirror(Some("http://test.mirror.localhost/download/"), None, None, "release"),
        "http://test.mirror.localhost/download/release/",
    );
}

#[test]
fn a_channel_named_outright_wins_over_the_base() {
    let mirrors = HashMap::from([("nightly".to_string(), "http://nightly.localhost".to_string())]);
    assert_eq!(
        get_node_mirror(Some("http://base.localhost"), None, Some(&mirrors), "nightly"),
        "http://nightly.localhost/",
    );
    assert_eq!(
        get_node_mirror(Some("http://base.localhost"), None, Some(&mirrors), "release"),
        "http://base.localhost/release/",
    );
}

#[test]
fn a_channel_overrides_the_base_it_is_named_beside() {
    assert_eq!(
        get_node_mirror(
            Some("http://base.localhost/download"),
            Some("http://nightly.localhost"),
            None,
            "nightly",
        ),
        "http://nightly.localhost/",
    );
    assert_eq!(
        get_node_mirror(Some("http://base.localhost/download"), None, None, "release"),
        "http://base.localhost/download/release/",
    );
}

/// `node-mirror:<channel>` is the older spelling of the same thing.
#[test]
fn the_canonical_channel_wins_over_the_older_spelling() {
    let mirrors = HashMap::from([("nightly".to_string(), "http://older.localhost".to_string())]);
    assert_eq!(
        get_node_mirror(None, Some("http://canonical.localhost"), Some(&mirrors), "nightly"),
        "http://canonical.localhost/",
    );
}
