use super::{LockedSource, read_locked_cargo_versions};
use semver::Version;

#[test]
fn locked_versions_are_read_per_source_without_workspace_members() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = dir.path().join("Cargo.lock");
    std::fs::write(
        &lockfile,
        concat!(
            "version = 4\n\n",
            "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n\n",
            "[[package]]\nname = \"lib\"\nversion = \"1.0.0\"\n",
            "source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
            "checksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\n",
            "[[package]]\nname = \"lib\"\nversion = \"1.1.0-fork\"\n",
            "source = \"git+https://example.test/lib?rev=abc#abc\"\n",
        ),
    )
    .unwrap();

    let registry = read_locked_cargo_versions(&lockfile, LockedSource::Registry).unwrap();
    let git = read_locked_cargo_versions(&lockfile, LockedSource::Git).unwrap();

    assert_eq!(registry.get("lib").unwrap(), &[Version::parse("1.0.0").unwrap()]);
    assert_eq!(git.get("lib").unwrap(), &[Version::parse("1.1.0-fork").unwrap()]);
    assert!(!registry.contains_key("app") && !git.contains_key("app"));
}
