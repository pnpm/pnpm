use super::{Version, prerelease_channel};

#[test]
fn identifies_prerelease_channels() {
    for (version, channel) in [
        ("3.0.0-rc.1", Some("rc")),
        ("0.0.0-next-2", Some("next")),
        ("1.0.0-beta2", Some("beta")),
        ("1.0.0-alpha.1.2", Some("alpha")),
        ("19.0.0-canary-e3ebcd54b-20240405", Some("canary")),
        ("1.0.0-next", Some("next")),
        ("1.0.0-1", None),
        ("1.0.0", None),
    ] {
        let parsed = Version::parse(version).expect("valid version");
        assert_eq!(prerelease_channel(&parsed), channel, "{version}");
    }
}
