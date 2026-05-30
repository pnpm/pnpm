use super::{PreferredPm, detect_preferred_pm};
use std::fs;
use tempfile::tempdir;

#[test]
fn defaults_to_npm_when_no_lockfile() {
    let dir = tempdir().unwrap();
    assert_eq!(detect_preferred_pm(dir.path()), PreferredPm::Npm);
}

#[test]
fn detects_pnpm_via_pnpm_lock_yaml() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    assert_eq!(detect_preferred_pm(dir.path()), PreferredPm::Pnpm);
}

#[test]
fn detects_yarn_via_yarn_lock() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "").unwrap();
    assert_eq!(detect_preferred_pm(dir.path()), PreferredPm::Yarn);
}

#[test]
fn detects_npm_via_package_lock_json() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
    assert_eq!(detect_preferred_pm(dir.path()), PreferredPm::Npm);
}

#[test]
fn detects_bun_via_either_lock_name() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("bun.lockb"), "").unwrap();
    assert_eq!(detect_preferred_pm(dir.path()), PreferredPm::Bun);

    let dir2 = tempdir().unwrap();
    fs::write(dir2.path().join("bun.lock"), "").unwrap();
    assert_eq!(detect_preferred_pm(dir2.path()), PreferredPm::Bun);
}

#[test]
fn pnpm_takes_precedence_over_yarn_and_npm() {
    // When multiple lockfiles are present we follow upstream's order
    // (pnpm wins) rather than newest-mtime or alphabetical.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
    fs::write(dir.path().join("yarn.lock"), "").unwrap();
    fs::write(dir.path().join("package-lock.json"), "").unwrap();
    assert_eq!(detect_preferred_pm(dir.path()), PreferredPm::Pnpm);
}

#[test]
fn pm_names_match_binary_invocations() {
    assert_eq!(PreferredPm::Pnpm.name(), "pnpm");
    assert_eq!(PreferredPm::Npm.name(), "npm");
    assert_eq!(PreferredPm::Yarn.name(), "yarn");
    assert_eq!(PreferredPm::Bun.name(), "bun");
}
