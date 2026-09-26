use super::{Version, prerelease_channel};

#[test]
fn identifies_dot_separated_prerelease_channels() {
    let version = Version::parse("3.0.0-rc.1").expect("valid version");
    assert_eq!(prerelease_channel(&version).as_deref(), Some("rc"));
}

#[test]
fn identifies_hyphenated_prerelease_channels() {
    let version = Version::parse("0.0.0-next-2").expect("valid version");
    assert_eq!(prerelease_channel(&version).as_deref(), Some("next"));
}
