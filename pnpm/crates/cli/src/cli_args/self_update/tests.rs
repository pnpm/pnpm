use super::{
    install_pnpm, is_installed_globally, package_manager_pin_specifier, refresh_global_shims,
    update_version_constraint, version_lt,
};
use crate::shim_dispatch::{ShimTarget, native_shim::install_native_shim_from, native_shim_target};
use std::{fs, path::Path};

#[test]
fn version_constraint_preserves_pinning_style() {
    // No prior constraint → the exact version.
    assert_eq!(update_version_constraint(None, "1.2.3"), "1.2.3");
    // Simple ranges that still satisfy are bumped in place, keeping the operator.
    assert_eq!(update_version_constraint(Some("^1.0.0"), "1.5.0"), "^1.5.0");
    assert_eq!(update_version_constraint(Some("~1.2.0"), "1.2.5"), "~1.2.5");
    // Complex ranges that still satisfy are left untouched; the lockfile pins
    // the exact version.
    assert_eq!(update_version_constraint(Some(">=1.0.0"), "1.5.0"), ">=1.0.0");
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
    // A range pin is rewritten to the new version, keeping the operator, so the
    // specifier is the range a later install reads back from the manifest.
    assert_eq!(package_manager_pin_specifier(false, Some("^12.0.0"), "12.1.0"), "^12.1.0");
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

fn seed_shim_and_new_engine(root: &Path) -> (install_pnpm::InstallPnpmResult, std::path::PathBuf) {
    let global_bin = root.join("bin");
    let install_dir = root.join("engine");
    fs::create_dir_all(&global_bin).unwrap();
    let executable = install_pnpm::pnpm_executable_path(&install_dir, "pnpm");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"new shim engine").unwrap();
    let old_engine = root.join("old-engine");
    fs::write(&old_engine, b"old shim engine").unwrap();
    let target = ShimTarget::Installed(root.join("node-release/bin/node"));
    install_native_shim_from(&old_engine, &global_bin, "node", &target).unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir,
        package_name: "pnpm",
        already_existed: false,
    };
    (installed, global_bin.join(format!("node{}", std::env::consts::EXE_SUFFIX)))
}

#[test]
fn self_update_republishes_global_shims_from_a_compatible_engine() {
    let root = tempfile::tempdir().unwrap();
    let (installed, node) = seed_shim_and_new_engine(root.path());
    let global_bin = root.path().join("bin");

    refresh_global_shims(&global_bin, &installed, "12.3.0").unwrap();

    assert_eq!(fs::read(node).unwrap(), b"new shim engine");
    assert_eq!(
        native_shim_target(&global_bin, "node").unwrap(),
        Some(ShimTarget::Installed(root.path().join("node-release/bin/node"))),
    );
}

/// The shims an earlier pnpm 12 wrote were shell scripts calling a
/// `.pnpm-shim-v1` dispatcher; a self-update turns them into native shims
/// and retires the dispatcher.
#[cfg(unix)]
#[test]
fn self_update_migrates_legacy_shell_shims() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let (installed, _) = seed_shim_and_new_engine(root.path());
    let global_bin = root.path().join("bin");
    let dispatcher = global_bin.join(".pnpm-shim-v1");
    fs::write(&dispatcher, b"old v12 engine").unwrap();
    let legacy_shim = global_bin.join("tool");
    fs::write(
        &legacy_shim,
        "#!/bin/sh\nexec \"$basedir/.pnpm-shim-v1\" --shim 'tool' -- \"$@\"\n# pnpm-shim-style=context-aware\n# cmd-shim-target=/global/tool/cli.js\n",
    )
    .unwrap();
    fs::set_permissions(&legacy_shim, fs::Permissions::from_mode(0o755)).unwrap();
    let legacy_virtual = global_bin.join("yarn");
    fs::write(
        &legacy_virtual,
        "#!/bin/sh\nexit 1\n# pnpm-shim-style=context-aware\n# cmd-shim-target=pkg:yarn\n",
    )
    .unwrap();
    fs::write(global_bin.join("direct"), "#!/bin/sh\nexec node\n# cmd-shim-target=/x/cli.js\n")
        .unwrap();

    refresh_global_shims(&global_bin, &installed, "12.3.0").unwrap();

    assert_eq!(fs::read(&legacy_shim).unwrap(), b"new shim engine");
    assert_eq!(
        native_shim_target(&global_bin, "tool").unwrap(),
        Some(ShimTarget::Installed("/global/tool/cli.js".into())),
    );
    assert_eq!(fs::read(&legacy_virtual).unwrap(), b"new shim engine");
    assert_eq!(
        native_shim_target(&global_bin, "yarn").unwrap(),
        Some(ShimTarget::Virtual("yarn".to_string())),
    );
    assert!(fs::read_to_string(global_bin.join("direct")).unwrap().starts_with("#!/bin/sh"));
    assert_eq!(native_shim_target(&global_bin, "direct").unwrap(), None);
    assert!(!dispatcher.exists());
}

#[test]
fn self_update_installs_no_shim_where_none_exists() {
    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir: root.path().join("engine"),
        package_name: "pnpm",
        already_existed: false,
    };

    refresh_global_shims(&global_bin, &installed, "12.3.0").unwrap();

    assert_eq!(fs::read_dir(&global_bin).unwrap().count(), 0);
}

#[test]
fn self_update_to_pnpm_without_native_shims_leaves_the_global_shims_alone() {
    let root = tempfile::tempdir().unwrap();
    let (_, node) = seed_shim_and_new_engine(root.path());
    let global_bin = root.path().join("bin");
    let installed = install_pnpm::InstallPnpmResult {
        install_dir: root.path().join("legacy-engine"),
        package_name: "pnpm",
        already_existed: false,
    };

    refresh_global_shims(&global_bin, &installed, "12.2.1").unwrap();

    assert_eq!(fs::read(node).unwrap(), b"old shim engine");
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
