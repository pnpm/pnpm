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
fn a_replacement_is_the_current_version_only_of_a_patched_crate_it_matches() {
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
            "source = \"git+https://example.test/lib?rev=abc#abc\"\n\n",
            "[[package]]\nname = \"tar\"\nversion = \"0.4.45\"\n\n",
            "[[package]]\nname = \"tar\"\nversion = \"0.4.40\"\n",
            "source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
            "checksum = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n",
        ),
    )
    .unwrap();

    let locked = read_locked_cargo_versions(&lockfile).unwrap();

    let version = |text| Version::parse(text).unwrap();
    assert!(!locked.registry.contains_key("app"));
    assert_eq!(locked.current(&lib(), false), Some(&version("1.0.0")));
    assert_eq!(locked.current(&lib(), true), Some(&version("1.5.0")), "git patch");
    let tar = DeclaredCargoDep {
        alias: "tar".to_string(),
        package_name: "tar".to_string(),
        version_req: VersionReq::parse("0.4").unwrap(),
        ..lib()
    };
    assert_eq!(locked.current(&tar, true), Some(&version("0.4.45")), "path patch");
    let lib_2 = DeclaredCargoDep { version_req: VersionReq::parse("2").unwrap(), ..lib() };
    assert_eq!(
        locked.current(&lib_2, true),
        Some(&version("1.0.0")),
        "a replacement that misses the requirement falls back to the registry entry",
    );
}
