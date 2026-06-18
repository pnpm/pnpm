use super::{AquaRegistryPackage, AquaVersionOverride, find_matching_override};

fn ripgrep_pkg() -> AquaRegistryPackage {
    let over = |constraint: &str, asset: &str| AquaVersionOverride {
        version_constraint: Some(constraint.to_string()),
        asset: Some(asset.to_string()),
        format: Some("tar.gz".to_string()),
        ..AquaVersionOverride::default()
    };
    AquaRegistryPackage {
        version_constraint: Some("false".to_string()),
        version_overrides: Some(vec![
            over(r#"semver("<= 0.1.0")"#, "old-{{.Version}}.tar.gz"),
            over(r#"Version == "1.0.0-beta""#, "beta-{{.Version}}.tar.gz"),
            over(r#"semver("<= 13.0.0")"#, "mid-{{.Version}}.tar.gz"),
            over("true", "latest-{{.Version}}.tar.gz"),
        ]),
        ..AquaRegistryPackage::default()
    }
}

#[test]
fn matches_the_catch_all_override_for_recent_versions() {
    let pkg = ripgrep_pkg();
    assert_eq!(find_matching_override(&pkg, "14.1.1").asset, Some("latest-{{.Version}}.tar.gz"));
}

#[test]
fn matches_semver_range_for_older_versions() {
    let pkg = ripgrep_pkg();
    assert_eq!(find_matching_override(&pkg, "0.0.5").asset, Some("old-{{.Version}}.tar.gz"));
}

#[test]
fn matches_mid_range_versions() {
    let pkg = ripgrep_pkg();
    assert_eq!(find_matching_override(&pkg, "12.0.0").asset, Some("mid-{{.Version}}.tar.gz"));
}

#[test]
fn matches_exact_version_constraints() {
    let pkg = ripgrep_pkg();
    assert_eq!(find_matching_override(&pkg, "1.0.0-beta").asset, Some("beta-{{.Version}}.tar.gz"));
}

#[test]
fn handles_v_prefixed_versions() {
    let pkg = ripgrep_pkg();
    assert_eq!(find_matching_override(&pkg, "v14.1.1").asset, Some("latest-{{.Version}}.tar.gz"));
}

#[test]
fn falls_back_to_the_base_package_when_no_overrides_exist() {
    let pkg = AquaRegistryPackage {
        asset: Some("tool-{{.Version}}.tar.gz".to_string()),
        ..AquaRegistryPackage::default()
    };
    assert_eq!(find_matching_override(&pkg, "1.2.3").asset, Some("tool-{{.Version}}.tar.gz"));
}
