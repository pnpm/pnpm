use super::{DeclaredCargoDep, read_locked_cargo_versions};
use pnpm_package_manifest::DependencyGroup;
use semver::{Version, VersionReq};
use std::path::PathBuf;

fn lib() -> DeclaredCargoDep {
    DeclaredCargoDep {
        alias: "lib".to_string(),
        package_name: "lib".to_string(),
        raw_spec: "1".to_string(),
        version_req: VersionReq::parse("1").unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "workspace.dependencies",
    }
}

#[test]
fn a_git_entry_is_the_current_version_only_of_a_patched_crate() {
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
            "[[package]]\nname = \"lib\"\nversion = \"1.5.0\"\n",
            "source = \"git+https://example.test/lib?rev=abc#abc\"\n",
        ),
    )
    .unwrap();

    let locked = read_locked_cargo_versions(&lockfile).unwrap();

    assert!(!locked.registry.contains_key("app") && !locked.git.contains_key("app"));
    assert_eq!(locked.current(&lib(), false), Some(&Version::parse("1.0.0").unwrap()));
    assert_eq!(locked.current(&lib(), true), Some(&Version::parse("1.5.0").unwrap()));
}
