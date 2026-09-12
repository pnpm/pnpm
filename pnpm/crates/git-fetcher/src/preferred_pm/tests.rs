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
fn a_berry_yarn_lockfile_pins_the_berry_line() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
    assert_eq!(
        detect_wanted_pm(dir.path(), None),
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some(">=2".to_string()), pinned: false },
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

/// A dependency's manifest is untrusted input and its pin reaches a
/// command line, so only a plain semver range is carried over. Everything
/// else — a URL, a dist-tag, a shell payload — leaves the version to pnpm.
#[test]
fn only_a_semver_range_is_carried_over_from_a_pin() {
    let dir = tempdir().unwrap();
    for reference in [
        "https://example.test/yarn.js",
        "latest",
        r#"1.0.0" & calc & ""#,
        "1.0.0; touch /tmp/pwned",
    ] {
        let manifest = serde_json::json!({ "packageManager": format!("yarn@{reference}") });
        assert_eq!(
            detect_wanted_pm(dir.path(), Some(&manifest)),
            // Nothing was pinned that pnpm can honor, so a host that has
            // Yarn keeps preparing the dependency with it.
            WantedPm { pm: PreferredPm::Yarn, version_spec: None, pinned: false },
            "reference was {reference}",
        );
    }
}

/// `devEngines.packageManager` holds a range rather than an exact version,
/// and it reaches the same command line.
#[test]
fn a_dev_engines_version_is_held_to_the_same_bar() {
    let dir = tempdir().unwrap();
    let pin = |version: &str| {
        let manifest = serde_json::json!({
            "devEngines": { "packageManager": { "name": "yarn", "version": version } },
        });
        detect_wanted_pm(dir.path(), Some(&manifest)).version_spec
    };
    assert_eq!(pin(">=2 <5").as_deref(), Some(">=2 <5"));
    assert_eq!(pin(r#"4" & calc & ""#), None);
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

/// A declaration that names the package manager but no version says
/// nothing about which release the dependency was tested against, so a
/// host that already has that package manager keeps doing the job.
#[test]
fn a_declaration_without_a_version_is_not_a_pin() {
    let dir = tempdir().unwrap();
    let manifest = serde_json::json!({
        "devEngines": { "packageManager": { "name": "yarn" } },
    });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Yarn, version_spec: None, pinned: false },
    );
}

/// Which Yarn line can read the lockfile is a constraint the manifest
/// does not have to repeat, so a declaration naming only Yarn still gets
/// the line from what the dependency ships.
#[test]
fn a_yarn_declaration_without_a_version_takes_the_line_from_the_lockfile() {
    for (lockfile, line) in
        [("__metadata:\n  version: 8\n", ">=2"), ("left-pad@^1.3.0:\n  version \"1.3.0\"\n", "1")]
    {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), lockfile).unwrap();
        let manifest = serde_json::json!({ "packageManager": "yarn" });
        assert_eq!(
            detect_wanted_pm(dir.path(), Some(&manifest)),
            WantedPm {
                pm: PreferredPm::Yarn,
                version_spec: Some(line.to_string()),
                // Inferred, not asked for: a host Yarn that can read the
                // lockfile is still allowed to do the work.
                pinned: false,
            },
            "{lockfile}",
        );
    }
}

/// Only the header is read, so a lockfile too large to hold in memory
/// still answers — and a `__metadata` block that is not in the header is
/// not a Berry stamp.
#[test]
fn the_line_is_read_from_the_lockfile_header() {
    let dir = tempdir().unwrap();
    let mut lockfile = String::from("__metadata:\n  version: 8\n");
    lockfile.push_str(&"# padding\n".repeat(200_000));
    fs::write(dir.path().join("yarn.lock"), &lockfile).unwrap();
    assert_eq!(detect_wanted_pm(dir.path(), None).version_spec.as_deref(), Some(">=2"));

    let dir = tempdir().unwrap();
    let mut lockfile = "# padding\n".repeat(200_000);
    lockfile.push_str("__metadata:\n  version: 8\n");
    fs::write(dir.path().join("yarn.lock"), &lockfile).unwrap();
    assert_eq!(detect_wanted_pm(dir.path(), None).version_spec.as_deref(), Some("1"));
}

/// The stamp is the `__metadata:` key, not a prefix: a lockfile holding
/// a key that merely starts like it was not written by Berry.
#[test]
fn a_lookalike_key_is_not_the_berry_stamp() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "__metadataEvil:\n  version: 8\n").unwrap();
    assert_eq!(detect_wanted_pm(dir.path(), None).version_spec.as_deref(), Some("1"));
}

/// A lockfile is a fetched artifact, not text pnpm validated: a byte no
/// encoding claims must not decide which Yarn prepares the package.
#[test]
fn a_lockfile_that_is_not_utf_8_still_reports_its_line() {
    let dir = tempdir().unwrap();
    let mut lockfile = b"# \xff\xfe not text\n".to_vec();
    lockfile.extend_from_slice(b"__metadata:\n  version: 8\n");
    fs::write(dir.path().join("yarn.lock"), &lockfile).unwrap();
    assert_eq!(detect_wanted_pm(dir.path(), None).version_spec.as_deref(), Some(">=2"));
}

/// A version the dependency did ask for outranks the lockfile's line.
#[test]
fn a_pinned_yarn_version_wins_over_the_lockfile_line() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
    let manifest = serde_json::json!({ "packageManager": "yarn@4.9.2" });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some("4.9.2".to_string()), pinned: true },
    );
}

/// A `devEngines` list declares alternatives, so an entry naming
/// something pnpm cannot provision is passed over rather than ending the
/// search.
#[test]
fn a_dev_engines_list_falls_through_to_an_entry_pnpm_can_provision() {
    let dir = tempdir().unwrap();
    let manifest = serde_json::json!({
        "devEngines": {
            "packageManager": [
                { "name": "cnpm", "version": "1.0.0" },
                { "name": "yarn", "version": "4.9.2" },
            ],
        },
    });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some("4.9.2".to_string()), pinned: true },
    );
}

/// `devEngines` outranks `packageManager`, but only for a package manager
/// pnpm can provision — an unknown one there must not bury the pin below
/// it.
#[test]
fn an_unknown_dev_engines_pin_leaves_package_manager_in_charge() {
    let dir = tempdir().unwrap();
    let manifest = serde_json::json!({
        "devEngines": { "packageManager": { "name": "cnpm", "version": "1.0.0" } },
        "packageManager": "yarn@4.9.2",
    });
    assert_eq!(
        detect_wanted_pm(dir.path(), Some(&manifest)),
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some("4.9.2".to_string()), pinned: true },
    );
}
