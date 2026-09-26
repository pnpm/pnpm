use super::{
    global_bin::{link_into_global_bin, refresh_global_shims},
    handler, install_pnpm, is_installed_globally, join_messages, version_lt,
};
use crate::{
    cli_args::self_update::project_pin::{
        NoUpgradeKind, implicit_latest_no_upgrade_message, package_manager_pin_specifier,
        update_version_constraint,
    },
    shim_dispatch::{ShimTarget, native_shim::install_native_shim_from, native_shim_target},
};
use pnpm_cmd_shim::generate_sh_shim;
use pnpm_config::Config;
use pnpm_reporter::SilentReporter;
use std::{
    fs,
    path::{Path, PathBuf},
};

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
    seed_global_engine_slot(global_dir, package_name, version, true);
}

fn seed_global_engine_slot(
    global_dir: &Path,
    package_name: &str,
    version: &str,
    with_executable: bool,
) {
    let install_dir = seed_engine_install_dir(global_dir, package_name, version, with_executable);
    pnpm_fs::force_symlink_dir(&install_dir, &global_dir.join(format!("hash-{version}"))).unwrap();
}

fn seed_engine_install_dir(
    global_dir: &Path,
    package_name: &str,
    version: &str,
    with_executable: bool,
) -> PathBuf {
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
    if with_executable {
        fs::write(install_pnpm::pnpm_executable_path(&install_dir, package_name), b"engine")
            .unwrap();
    }
    install_dir
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

    // The standalone install script installs a v12 engine as `@pnpm/exe`, while
    // `pnpm_package_to_install` resolves v12 to `pnpm`. The install still counts.
    seed_global_engine(global_dir, "@pnpm/exe", "12.3.4");
    assert!(is_installed_globally(Some(global_dir), "12.3.4").unwrap());
    // A `pnpm` group at another version does not hide the matching `@pnpm/exe` one.
    seed_global_engine(global_dir, "pnpm", "12.4.0");
    assert!(is_installed_globally(Some(global_dir), "12.3.4").unwrap());

    // A group recording the target version but missing its executable is not
    // the engine yet, so the update proceeds and relinks it.
    seed_global_engine_slot(global_dir, "@pnpm/exe", "12.5.0", false);
    assert!(!is_installed_globally(Some(global_dir), "12.5.0").unwrap());
}

#[test]
fn self_update_replaces_the_engine_installed_under_the_other_alias() {
    // pnpm/pnpm#14709
    let root = tempfile::tempdir().unwrap();
    let global_dir = root.path().join("global");
    seed_global_engine(&global_dir, "@pnpm/exe", "12.3.4");
    fs::create_dir_all(global_dir.join("tool/node_modules")).unwrap();
    fs::write(global_dir.join("tool/package.json"), r#"{"dependencies":{"typescript":"6.0.0"}}"#)
        .unwrap();
    pnpm_fs::force_symlink_dir(&global_dir.join("tool"), &global_dir.join("hash-tool")).unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir: seed_engine_install_dir(&global_dir, "pnpm", "12.4.0", true),
        package_name: "pnpm",
        already_existed: false,
    };
    let config = Config {
        global_bin: Some(root.path().join("bin")),
        global_pkg_dir: Some(global_dir.clone()),
        ..Config::default()
    };
    fs::create_dir_all(root.path().join("bin")).unwrap();

    link_into_global_bin(&config, &installed, "12.4.0").unwrap();

    let mut groups: Vec<_> = pnpm_global::scan_global_packages(&global_dir)
        .unwrap()
        .into_iter()
        .map(|group| group.dependencies)
        .collect();
    groups.sort();
    assert_eq!(
        groups,
        [
            vec![("pnpm".to_string(), "12.4.0".to_string())],
            vec![("typescript".to_string(), "6.0.0".to_string())],
        ],
    );
}

#[test]
fn self_update_rewrites_the_home_shim_without_parent_segments() {
    let root = tempfile::tempdir().unwrap();
    let global_dir = root.path().join("global");
    let install_dir = seed_engine_install_dir(&global_dir, "pnpm", "12.4.0", true);
    let package_dir = install_pnpm::package_dir(&install_dir, "pnpm");
    fs::write(
        package_dir.join("package.json"),
        r#"{"name":"pnpm","version":"12.4.0","bin":{"pnpm":"pnpm"}}"#,
    )
    .unwrap();
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = install_pnpm::pnpm_executable_path(&install_dir, "pnpm");
    let shim_path = bin.join("pnpm");
    fs::write(&shim_path, generate_sh_shim(&executable, &shim_path, None, &[], None)).unwrap();
    let installed = install_pnpm::InstallPnpmResult {
        install_dir,
        package_name: "pnpm",
        already_existed: false,
    };
    let config =
        Config { global_bin: Some(bin), global_pkg_dir: Some(global_dir), ..Config::default() };

    link_into_global_bin(&config, &installed, "12.4.0").unwrap();

    let shim = fs::read_to_string(&shim_path).unwrap();
    let expected = executable.to_string_lossy().replace('\\', "/");
    assert!(shim.contains(&format!("\"{expected}\"")), "{shim}");
    assert!(!shim.contains("$basedir/../"), "{shim}");
    assert!(!shim.contains("/../"), "{shim}");
}

#[test]
fn a_project_pin_message_does_not_hide_the_global_switch() {
    // Guards pnpm/pnpm#14747: `self-update` in a project already pinned to the
    // resolved version still moves the global install forward, and has to say so.
    assert_eq!(
        join_messages(Some("pinned".to_string()), Some("switched".to_string())),
        Some("pinned\nswitched".to_string()),
    );
    assert_eq!(join_messages(Some("pinned".to_string()), None), Some("pinned".to_string()));
    assert_eq!(join_messages(None, Some("switched".to_string())), Some("switched".to_string()));
    assert_eq!(join_messages(None, None), None);
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

#[test]
fn implicit_latest_message_mentions_minimum_release_age_when_registry_latest_is_not_older() {
    let message =
        implicit_latest_no_upgrade_message(NoUpgradeKind::Project, "9.1.0", "9.0.0", Some("9.1.0"));
    assert!(message.contains("minimumReleaseAge") && !message.contains("downgrade"), "{message}");
    let active =
        implicit_latest_no_upgrade_message(NoUpgradeKind::Active, "9.1.0", "9.0.0", Some("9.1.0"));
    assert!(active.contains("minimumReleaseAge") && !active.contains("downgrade"), "{active}");
}

#[test]
fn implicit_latest_message_still_offers_downgrade_when_registry_latest_is_older() {
    let message = implicit_latest_no_upgrade_message(
        NoUpgradeKind::Active,
        "9.0.0",
        "8.15.0",
        Some("8.15.0"),
    );
    assert!(message.contains("downgrade") && !message.contains("minimumReleaseAge"), "{message}");
}

#[test]
fn implicit_latest_message_names_both_versions_when_registry_latest_is_older_but_immature() {
    let message = implicit_latest_no_upgrade_message(
        NoUpgradeKind::Project,
        "10.0.0",
        "9.0.0",
        Some("9.5.0"),
    );
    assert!(
        message.contains(r#""latest" version on the registry (v9.5.0)"#)
            && message.contains("minimumReleaseAge is v9.0.0")
            && message.contains("downgrade"),
        "{message}",
    );
}

/// A registry serving `mature` published long ago and `fresh` published now,
/// with `latest` on `fresh`.
async fn registry_with_fresh_latest(mature: &str, fresh: &str) -> mockito::ServerGuard {
    let dist = |version: &str| {
        format!(
            r#"{{"name":"pnpm","version":"{version}","dist":{{"shasum":"0000000000000000000000000000000000000000","tarball":"https://registry/pnpm-{version}.tgz"}}}}"#,
        )
    };
    let body = format!(
        r#"{{"name":"pnpm","dist-tags":{{"latest":"{fresh}"}},"time":{{"{mature}":"2024-01-10T08:30:00.000Z","{fresh}":"{now}"}},"versions":{{"{mature}":{mature_dist},"{fresh}":{fresh_dist}}}}}"#,
        now = chrono::Utc::now().to_rfc3339(),
        mature_dist = dist(mature),
        fresh_dist = dist(fresh),
    );
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/pnpm")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;
    server
}

/// Run an implicit `self-update` in a project pinned to `pin`, under a
/// one-day `minimumReleaseAge`, and return its message and the manifest it
/// leaves behind.
async fn implicit_self_update_of_pin(server: &mockito::ServerGuard, pin: &str) -> (String, String) {
    let root = tempfile::tempdir().expect("tempdir");
    let manifest = format!(r#"{{"packageManager":"pnpm@{pin}"}}"#);
    fs::write(root.path().join("package.json"), &manifest).expect("write package.json");
    let mut config = Config {
        minimum_release_age: Some(24 * 60),
        cache_dir: root.path().join("cache"),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = format!("{}/", server.url());
    let config: &'static Config = Box::leak(Box::new(config));

    let message = handler::<SilentReporter>(None, config, root.path()).await
        .expect("the refusal is not an error")
        .expect("the refusal prints a message");
    let manifest_after = fs::read_to_string(root.path().join("package.json")).expect("read");
    (message, manifest_after)
}

#[tokio::test]
async fn implicit_self_update_names_the_cutoff_when_the_pin_is_the_immature_latest() {
    let server = registry_with_fresh_latest("900.0.0", "900.1.0").await;

    let (message, manifest) = implicit_self_update_of_pin(&server, "900.1.0").await;

    assert_eq!(
        message,
        "The current project is set to use pnpm v900.1.0. The latest version that meets minimumReleaseAge is v900.0.0. v900.1.0 on the registry is still within the cutoff. No update performed.",
    );
    assert_eq!(manifest, r#"{"packageManager":"pnpm@900.1.0"}"#);
}

#[tokio::test]
async fn implicit_self_update_names_both_versions_when_the_immature_latest_is_older_than_the_pin() {
    let server = registry_with_fresh_latest("900.0.0", "900.5.0").await;

    let (message, manifest) = implicit_self_update_of_pin(&server, "901.0.0").await;

    assert_eq!(
        message,
        r#"The current project is set to use pnpm v901.0.0, which is newer than the "latest" version on the registry (v900.5.0). The latest version that meets minimumReleaseAge is v900.0.0. No update performed. Run "pnpm self-update latest" to downgrade."#,
    );
    assert_eq!(manifest, r#"{"packageManager":"pnpm@901.0.0"}"#);
}
