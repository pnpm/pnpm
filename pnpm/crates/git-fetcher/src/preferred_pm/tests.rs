use super::{PreferredPm, WantedPm, detect_preferred_pm, detect_wanted_pm};
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

/// The Classic/Berry split is the one the lockfile has to decide: Berry
/// stamps `__metadata` into every lockfile it writes, and neither line
/// can install the other's.
#[test]
fn a_classic_yarn_lockfile_pins_yarn_1() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "left-pad@^1.3.0:\n  version \"1.3.0\"\n").unwrap();
    assert_eq!(
        detect_wanted_pm(dir.path(), None),
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some("1".to_string()), pinned: false },
    );
}

#[test]
fn a_berry_yarn_lockfile_leaves_the_version_open() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
    assert_eq!(
        detect_wanted_pm(dir.path(), None),
        WantedPm { pm: PreferredPm::Yarn, version_spec: None, pinned: false },
    );
}

/// Every other package manager reads the lockfiles its older releases
/// wrote, so nothing has to be pinned from the lockfile's shape.
#[test]
fn other_lockfiles_leave_the_version_open() {
    for lockfile in ["pnpm-lock.yaml", "package-lock.json", "bun.lock"] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(lockfile), "").unwrap();
        assert_eq!(detect_wanted_pm(dir.path(), None).version_spec, None, "{lockfile}");
    }
}

#[test]
fn a_package_manager_pin_wins_over_the_lockfile() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "").unwrap();
    let manifest = serde_json::json!({ "packageManager": "yarn@4.9.2+sha224.abc" });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some("4.9.2".to_string()), pinned: true },
    );
}

#[test]
fn a_dev_engines_pin_wins_over_the_lockfile() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package-lock.json"), "").unwrap();
    let manifest = serde_json::json!({
        "devEngines": { "packageManager": { "name": "pnpm", "version": "10.5.0" } },
    });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Pnpm, version_spec: Some("10.5.0".to_string()), pinned: true },
    );
}

/// A dependency's manifest is untrusted input: a reference that is a URL
/// rather than a version leaves the version to pnpm.
#[test]
fn a_url_reference_does_not_become_a_version() {
    let dir = tempdir().unwrap();
    let manifest = serde_json::json!({ "packageManager": "yarn@https://example.test/yarn.js" });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Yarn, version_spec: None, pinned: true },
    );
}

/// A pin naming something pnpm cannot provision falls back to the
/// lockfile rather than failing the install.
#[test]
fn an_unknown_package_manager_pin_is_ignored() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
    let manifest = serde_json::json!({ "packageManager": "cnpm@1.0.0" });
    assert_eq!(detect_wanted_pm(dir.path(), Some(&manifest)).pm, PreferredPm::Pnpm);
}
