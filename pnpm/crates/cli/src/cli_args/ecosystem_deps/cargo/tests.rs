use super::{latest_cargo_target, latest_requirement};
use crate::cli_args::ecosystem_deps::cargo_manifest_ops::DeclaredCargoDep;
use pnpm_package_manifest::DependencyGroup;
use semver::Version;
use std::path::PathBuf;

#[test]
fn latest_cargo_target_does_not_downgrade_a_prerelease() {
    let current = Version::parse("7.0.0-rc2").unwrap();
    let latest = Version::parse("6.2.1").unwrap();

    assert_eq!(latest_cargo_target(&current, latest), current);
}

#[test]
fn latest_requirement_preserves_renamed_alias_ranges() {
    let dependency = DeclaredCargoDep {
        alias: "tower-0-4".to_string(),
        package_name: "tower".to_string(),
        raw_spec: "0.4.13".to_string(),
        version_req: "0.4.13".parse().unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "dependencies",
    };

    assert_eq!(latest_requirement(&dependency), Some(&dependency.version_req));
}

#[test]
fn latest_requirement_preserves_prerelease_ranges() {
    let dependency = DeclaredCargoDep {
        alias: "dashmap".to_string(),
        package_name: "dashmap".to_string(),
        raw_spec: "7.0.0-rc2".to_string(),
        version_req: "7.0.0-rc2".parse().unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "dependencies",
    };

    assert_eq!(latest_requirement(&dependency), Some(&dependency.version_req));
}

#[test]
fn latest_requirement_ignores_build_metadata_when_detecting_prereleases() {
    let dependency = DeclaredCargoDep {
        alias: "dashmap".to_string(),
        package_name: "dashmap".to_string(),
        raw_spec: "1.1.8+spec-1.1.0".to_string(),
        version_req: "1.1.8+spec-1.1.0".parse().unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "dependencies",
    };

    assert_eq!(latest_requirement(&dependency), None);
}
