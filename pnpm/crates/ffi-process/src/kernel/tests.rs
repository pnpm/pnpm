use super::{darwin_major_version, release};

/// Pins that the release the kernel reports is the one the gate parses, so
/// a flag gate cannot read a major version the running kernel does not
/// have. macOS 11 is Darwin 20, and pnpm's own support floor is well past
/// it, so any machine running this test is at least there.
#[test]
fn the_darwin_major_version_comes_from_the_release() {
    let release = release().expect("uname reports a release");
    let major: u32 = release
        .split('.')
        .next()
        .expect("a release has a first component")
        .parse()
        .expect("a Darwin release starts with its major version");
    assert_eq!(darwin_major_version(), Some(major));
    assert!(major >= 20, "unexpectedly old Darwin: {release}");
}
