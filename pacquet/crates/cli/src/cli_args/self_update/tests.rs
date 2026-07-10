use super::{install_pnpm, is_installed_globally, update_version_constraint, version_lt};
use std::{fs, path::Path};

#[test]
fn version_constraint_preserves_pinning_style() {
    // No prior constraint → the exact version.
    assert_eq!(update_version_constraint(None, "1.2.3"), "1.2.3");
    // A range that still satisfies the new version is left untouched; the
    // lockfile pins the exact version.
    assert_eq!(update_version_constraint(Some("^1.0.0"), "1.5.0"), "^1.0.0");
    // A range that no longer satisfies is rewritten in its own style.
    assert_eq!(update_version_constraint(Some("^1.0.0"), "2.0.0"), "^2.0.0");
    assert_eq!(update_version_constraint(Some("~1.0.0"), "2.0.0"), "~2.0.0");
    // An exact pin stays exact.
    assert_eq!(update_version_constraint(Some("1.0.0"), "2.0.0"), "2.0.0");
    // A complex multi-comparator range falls back to a caret range.
    assert_eq!(update_version_constraint(Some(">=1.0.0 <2.0.0"), "3.0.0"), "^3.0.0");
}

fn seed_global_engine(global_dir: &Path, package_name: &str, version: &str) {
    let install_dir = global_dir.join(format!("pnpm-{version}"));
    let package_dir = install_pnpm::package_dir(&install_dir, package_name);
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(
        install_dir.join("package.json"),
        format!(r#"{{"dependencies":{{"{package_name}":"{version}"}}}}"#),
    )
    .unwrap();
    fs::write(
        package_dir.join("package.json"),
        format!(r#"{{"name":"{package_name}","version":"{version}"}}"#),
    )
    .unwrap();
    pacquet_fs::force_symlink_dir(&install_dir, &global_dir.join(format!("hash-{version}")))
        .unwrap();
}

#[test]
fn is_installed_globally_requires_a_matching_global_install() {
    assert!(!is_installed_globally(None, "11.0.0").unwrap());

    let global_dir = tempfile::tempdir().unwrap();
    let global_dir = global_dir.path();
    assert!(!is_installed_globally(Some(global_dir), "11.0.0").unwrap());

    seed_global_engine(global_dir, "@pnpm/exe", "11.0.0");
    assert!(is_installed_globally(Some(global_dir), "11.0.0").unwrap());
    // A different target version of the same engine package is not a match.
    assert!(!is_installed_globally(Some(global_dir), "11.1.0").unwrap());
}

#[test]
fn version_lt_compares_semver() {
    assert!(version_lt("1.0.0", "2.0.0"));
    assert!(version_lt("12.0.0-alpha.0", "12.0.0"));
    assert!(!version_lt("2.0.0", "1.0.0"));
    assert!(!version_lt("1.0.0", "1.0.0"));
    // Unparsable input compares as not-less-than (never downgrades).
    assert!(!version_lt("not-a-version", "1.0.0"));
}
