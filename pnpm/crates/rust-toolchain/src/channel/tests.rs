use super::{Channel, ChannelName};
use pretty_assertions::assert_eq;

fn version(major: u64, minor: u64, patch: Option<u64>) -> Channel {
    Channel::Version { major, minor, patch }
}

fn named(name: ChannelName, date: Option<&str>) -> Channel {
    Channel::Named { name, date: date.map(str::to_string) }
}

#[test]
fn parses_the_channels_the_distribution_server_publishes() {
    assert_eq!(Channel::parse("1.90.0"), Some(version(1, 90, Some(0))));
    assert_eq!(Channel::parse(" 1.90\n"), Some(version(1, 90, None)));
    assert_eq!(Channel::parse("stable"), Some(named(ChannelName::Stable, None)));
    assert_eq!(
        Channel::parse("nightly-2025-01-01"),
        Some(named(ChannelName::Nightly, Some("2025-01-01"))),
    );
    assert_eq!(Channel::parse("beta"), Some(named(ChannelName::Beta, None)));
}

#[test]
fn refuses_names_the_distribution_server_does_not_publish() {
    for name in [
        "",
        "1",
        "1.90.0.1",
        "01.90.0",
        "1.90.x",
        "nightly-2025-1-1",
        "nightly-x86_64-unknown-linux-gnu",
        "my-linked-toolchain",
        "../stable",
    ] {
        assert_eq!(Channel::parse(name), None, "{name:?}");
    }
}

#[test]
fn names_the_manifest_of_a_channel() {
    assert_eq!(version(1, 90, Some(0)).manifest_path(), "dist/channel-rust-1.90.0.toml");
    assert_eq!(version(1, 90, None).manifest_path(), "dist/channel-rust-1.90.toml");
    assert_eq!(named(ChannelName::Stable, None).manifest_path(), "dist/channel-rust-stable.toml");
    assert_eq!(
        named(ChannelName::Nightly, Some("2025-01-01")).manifest_path(),
        "dist/2025-01-01/channel-rust-nightly.toml",
    );
}

#[test]
fn only_versions_and_dated_channels_are_pinned() {
    assert!(version(1, 90, Some(0)).is_pinned());
    assert!(named(ChannelName::Nightly, Some("2025-01-01")).is_pinned());
    assert!(!version(1, 90, None).is_pinned());
    assert!(!named(ChannelName::Stable, None).is_pinned());
}

#[test]
fn a_moving_channel_accepts_the_releases_it_resolves_to() {
    let stable = named(ChannelName::Stable, None);
    assert!(stable.accepts(&version(1, 90, Some(0))));
    assert!(!stable.accepts(&named(ChannelName::Nightly, Some("2025-01-01"))));

    let line = version(1, 90, None);
    assert!(line.accepts(&version(1, 90, Some(2))));
    assert!(!line.accepts(&version(1, 91, Some(0))));

    let nightly = named(ChannelName::Nightly, None);
    assert!(nightly.accepts(&named(ChannelName::Nightly, Some("2025-01-01"))));
    assert!(!nightly.accepts(&named(ChannelName::Beta, Some("2025-01-01"))));
}
