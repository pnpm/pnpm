use super::{
    install_pnpm, is_installed_globally, package_manager_pin_specifier,
    refresh_global_shim_dispatcher, update_version_constraint, version_lt,
};
use pnpm_cmd_shim::CONTEXT_AWARE_DISPATCHER_NAME;
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
    pnpm_fs::force_symlink_dir(&install_dir, &global_dir.join(format!("hash-{version}"))).unwrap();
}

#[test]
fn pin_specifier_records_the_resolved_pin_not_the_cli_dist_tag() {
    // Guards the `self-update next-12` regression: recording the dist-tag
    // instead of the resolved pin desyncs the lockfile from the manifest and
    // breaks the next `--frozen-lockfile` install.
    assert_eq!(
        package_manager_pin_specifier(false, Some("12.0.0-alpha.9"), "12.0.0-alpha.10"),
        "12.0.0-alpha.10",
    );
    // A range pin is preserved (the lockfile pins the exact version), so the
    // specifier is the range a later install reads back from the manifest.
    assert_eq!(package_manager_pin_specifier(false, Some("^12.0.0"), "12.1.0"), "^12.0.0");
    // A legacy `packageManager` pin is always exact.
    assert_eq!(package_manager_pin_specifier(true, Some("^12.0.0"), "12.1.0"), "12.1.0");
    // No prior constraint → the resolved version.
    assert_eq!(package_manager_pin_specifier(false, None, "12.1.0"), "12.1.0");
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

#[test]
fn self_update_refreshes_an_existing_v1_dispatcher_from_the_v12_engine() {
    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("bin");
    let install_dir = root.path().join("engine");
    fs::create_dir_all(&global_bin).unwrap();
    let executable = install_pnpm::pnpm_executable_path(&install_dir, "pnpm");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"new v12 engine").unwrap();
    let dispatcher = global_shim_dispatcher_path(&global_bin);
    fs::write(&dispatcher, b"old v12 engine").unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir,
        package_name: "pnpm",
        already_existed: false,
    };

    refresh_global_shim_dispatcher(&global_bin, &installed, "12.0.0-alpha.1").unwrap();

    assert_eq!(fs::read(dispatcher).unwrap(), b"new v12 engine");
}

#[cfg(windows)]
#[test]
fn self_update_refreshes_node_when_it_is_the_v1_dispatcher() {
    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("bin");
    let install_dir = root.path().join("engine");
    fs::create_dir_all(&global_bin).unwrap();
    let executable = install_pnpm::pnpm_executable_path(&install_dir, "pnpm");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"new v12 engine").unwrap();
    let dispatcher = global_shim_dispatcher_path(&global_bin);
    fs::write(&dispatcher, b"old v12 engine").unwrap();
    let node = global_bin.join("node.exe");
    fs::hard_link(&dispatcher, &node).unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir,
        package_name: "pnpm",
        already_existed: false,
    };

    refresh_global_shim_dispatcher(&global_bin, &installed, "12.0.0").unwrap();

    assert_eq!(fs::read(dispatcher).unwrap(), b"new v12 engine");
    assert_eq!(fs::read(node).unwrap(), b"new v12 engine");
}

#[test]
fn self_update_leaves_a_missing_v1_dispatcher_absent() {
    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir: root.path().join("engine"),
        package_name: "pnpm",
        already_existed: false,
    };

    refresh_global_shim_dispatcher(&global_bin, &installed, "12.0.0").unwrap();

    assert!(!global_shim_dispatcher_path(&global_bin).exists());
}

#[test]
fn self_update_to_pre_v12_preserves_the_v1_dispatcher() {
    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let dispatcher = global_shim_dispatcher_path(&global_bin);
    fs::write(&dispatcher, b"v12 dispatcher").unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir: root.path().join("legacy-engine"),
        package_name: "@pnpm/exe",
        already_existed: false,
    };

    refresh_global_shim_dispatcher(&global_bin, &installed, "11.10.0").unwrap();

    assert_eq!(fs::read(dispatcher).unwrap(), b"v12 dispatcher");
}

fn global_shim_dispatcher_path(global_bin: &Path) -> std::path::PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    global_bin.join(format!("{CONTEXT_AWARE_DISPATCHER_NAME}{suffix}"))
}

/// The engine is a native binary, so building a runnable and a non-runnable one
/// means writing real executables — hence the unix gate, matching the `/bin/sh`
/// shims the rest of this crate's tests use.
#[cfg(unix)]
fn seed_engine_executable(install_dir: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    let package_dir = install_pnpm::package_dir(install_dir, "@pnpm/exe");
    fs::create_dir_all(&package_dir).unwrap();
    let executable = package_dir.join("pnpm");
    fs::write(&executable, contents).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn assert_pnpm_runs_accepts_an_engine_that_executes() {
    let global_dir = tempfile::tempdir().unwrap();
    let install_dir = global_dir.path().join("1");
    seed_engine_executable(&install_dir, "#!/bin/sh\nexit 0\n");

    install_pnpm::assert_pnpm_runs(&install_dir, "@pnpm/exe", "1.2.3").unwrap();
}

#[cfg(unix)]
#[test]
fn assert_pnpm_runs_rejects_the_placeholder_left_by_a_missing_native() {
    let global_dir = tempfile::tempdir().unwrap();
    let install_dir = global_dir.path().join("1");
    // Exactly what @pnpm/exe ships when its platform package carries no binary:
    // the wrapper is present and executable, but it is not a program.
    seed_engine_executable(&install_dir, "This file intentionally left blank");

    let err = install_pnpm::assert_pnpm_runs(&install_dir, "@pnpm/exe", "1.2.3").unwrap_err();

    assert!(err.to_string().contains("cannot run"), "{err}");
}

#[cfg(unix)]
#[test]
fn assert_pnpm_runs_reports_the_exit_code_of_an_engine_that_fails() {
    let global_dir = tempfile::tempdir().unwrap();
    let install_dir = global_dir.path().join("1");
    seed_engine_executable(&install_dir, "#!/bin/sh\nexit 1\n");

    let err = install_pnpm::assert_pnpm_runs(&install_dir, "@pnpm/exe", "1.2.3").unwrap_err();

    assert!(err.to_string().contains("exited with code 1"), "{err}");
}
