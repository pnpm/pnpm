use super::read_locked_cargo_versions;
use semver::Version;

#[test]
fn locked_versions_include_git_replacements_and_leave_out_workspace_members() {
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

    let locked = read_locked_cargo_versions(&lockfile).unwrap();

    assert_eq!(
        locked.get("lib").unwrap(),
        &[Version::parse("1.0.0").unwrap(), Version::parse("1.1.0-fork").unwrap()],
    );
    assert!(!locked.contains_key("app"));
}
