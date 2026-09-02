use super::{
    CliArgs, CliCommand, KeyIssueReporting, PackageManagerToSync, PreCommandInput, PreCommandPlan,
    SwitchInput, SwitchProcessState, SwitchSource, frozen_lockfile_flag,
    pre_command_plan_from_input, switch_target,
};
use crate::{boolean_negations::with_boolean_negations, config_overrides::ConfigOverrides};
use clap::{CommandFactory, FromArgMatches};
use pnpm_config::{Config, PNPM_VERSION, PmOnFail};
use pnpm_reporter::{Reporter, SilentReporter};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

#[test]
fn version_argv_reads_dir_auth_file_and_command_forms() {
    struct Case {
        name: &'static str,
        argv: &'static [&'static str],
        dir: &'static str,
        npmrc_auth_file: Option<&'static str>,
        command: Option<&'static str>,
    }

    let cases = [
        Case {
            name: "separate long dir and equals auth file",
            argv: &["pnpm", "--dir", "/tmp/project", "--npmrc-auth-file=auth.ini", "--version"],
            dir: "/tmp/project",
            npmrc_auth_file: Some("auth.ini"),
            command: None,
        },
        Case {
            name: "short dir",
            argv: &["pnpm", "-C", "/tmp/short-dir", "--version"],
            dir: "/tmp/short-dir",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "equals dir and userconfig alias",
            argv: &["pnpm", "--dir=/tmp/equals-dir", "--userconfig", "user.ini", "--version"],
            dir: "/tmp/equals-dir",
            npmrc_auth_file: Some("user.ini"),
            command: None,
        },
        Case {
            name: "prefix alias of dir",
            argv: &["pnpm", "--prefix", "/tmp/prefix-dir", "--version"],
            dir: "/tmp/prefix-dir",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "equals prefix alias of dir",
            argv: &["pnpm", "--prefix=/tmp/equals-prefix", "--version"],
            dir: "/tmp/equals-prefix",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "separator stops command detection",
            argv: &["pnpm", "--dir=/tmp/separator", "--", "run"],
            dir: "/tmp/separator",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "value-taking global option is skipped",
            argv: &["pnpm", "--filter", "pkg", "--reporter", "append-only", "install"],
            dir: ".",
            npmrc_auth_file: None,
            command: Some("install"),
        },
        Case {
            name: "store directory value is not mistaken for the command",
            argv: &["pnpm", "--store", "/tmp/store", "--prefix", "/tmp/scanned", "--version"],
            dir: "/tmp/scanned",
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "canonical store directory value is not mistaken for the command",
            argv: &["pnpm", "--store-dir", "/tmp/store", "--dir", "/tmp/scanned", "--version"],
            dir: "/tmp/scanned",
            npmrc_auth_file: None,
            command: None,
        },
    ];

    for case in cases {
        let argv = case.argv.iter().copied().map(OsString::from).collect::<Vec<_>>();
        let input = SwitchInput::from_version_argv(&argv);

        assert_eq!(input.dir, PathBuf::from(case.dir), "case: {}", case.name);
        assert_eq!(
            input.npmrc_auth_file,
            case.npmrc_auth_file.map(PathBuf::from),
            "case: {}",
            case.name,
        );
        assert_eq!(input.command.as_deref(), case.command, "case: {}", case.name);
    }

    let input = SwitchInput::from_version_argv(&[
        OsString::from("pnpm"),
        OsString::from("--state-dir"),
        OsString::from("/tmp/state"),
        OsString::from("--version"),
    ]);
    assert_eq!(input.state_dir.as_deref(), Some(Path::new("/tmp/state")));
}

#[test]
fn pre_command_plan_reports_a_pnpm_pin_corepack_prevents_switching() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: true },
    );

    // Corepack owns version selection, so the mismatch is reported instead
    // of switched.
    let error = plan.expect_err("expected the package manager check to fail");
    dbg!(&error);
    assert!(
        error.to_string().contains("This project is configured to use 9.3.0 of pnpm"),
        "unexpected error: {error:?}",
    );
}

#[test]
fn pre_command_plan_accepts_a_pnpm_pin_when_version_switching_is_turned_off() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected switch plan");
}

#[test]
fn pre_command_plan_reports_a_project_pinned_to_another_package_manager() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"yarn@4.0.0"}"#);

    let error = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect_err("expected the package manager check to fail");

    dbg!(&error);
    assert!(
        error.to_string().contains("This project is configured to use yarn"),
        "unexpected error: {error:?}",
    );
}

#[test]
fn pre_command_plan_checks_the_runtime_pinned_by_the_root_manifest() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"runtime":{"name":"node","version":"99999.0.0","onFail":"error"}}}"#,
    );

    let error = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState::current(),
    )
    .expect_err("expected the runtime check to fail");

    dbg!(&error);
    assert!(
        error.to_string().contains("This project requires Node.js 99999.0.0"),
        "unexpected error: {error:?}",
    );
}

#[test]
fn pre_command_plan_skips_the_runtime_check_for_global_commands() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"runtime":{"name":"node","version":"99999.0.0","onFail":"error"}}}"#,
    );

    let plan = pre_command_plan_from_input(
        &PreCommandInput { global: true, ..pre_command_input(root.path()) },
        &ConfigOverrides::default(),
        SwitchProcessState::current(),
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected switch plan");
}

#[test]
fn pre_command_plan_records_a_pin_the_running_pnpm_already_satisfies() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    // No switch is needed, but the pin still has to reach the lockfile.
    let Some(PreCommandPlan::SyncEnvLockfile(sync)) = plan else {
        panic!("expected an env lockfile sync, got {plan:?}");
    };
    assert_eq!(
        sync.package_manager,
        PackageManagerToSync {
            specifier: PNPM_VERSION.to_string(),
            version: PNPM_VERSION.to_string(),
        },
    );
}

#[test]
fn pre_command_plan_records_a_pin_that_only_warns() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        &format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{PNPM_VERSION}","onFail":"warn"}}}}}}"#,
        ),
    );

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(
        matches!(plan, Some(PreCommandPlan::SyncEnvLockfile(_))),
        "expected an env lockfile sync, got {plan:?}",
    );
}

#[test]
fn pre_command_plan_leaves_the_env_lockfile_sync_to_the_install_pipeline() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    let plan = pre_command_plan_from_input(
        &PreCommandInput { syncs_env_lockfile_in_pipeline: true, ..pre_command_input(root.path()) },
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected pre-command plan: {plan:?}");
}

#[test]
fn pre_command_plan_skips_the_env_lockfile_sync_when_the_lockfile_is_up_to_date() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);
    write_lockfile(root.path(), &locked_package_manager(PNPM_VERSION, PNPM_VERSION));

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected pre-command plan: {plan:?}");
}

#[test]
fn pre_command_plan_records_a_pin_whose_specifier_the_lockfile_no_longer_matches() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);
    // The locked version still satisfies the pin — so there is nothing to
    // switch to, and the lockfile the switch lookup already read is the one
    // the sync decision reuses — but it was resolved from a wider specifier.
    write_lockfile(root.path(), &locked_package_manager(">=0.0.0", PNPM_VERSION));

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(
        matches!(plan, Some(PreCommandPlan::SyncEnvLockfile(_))),
        "expected an env lockfile sync, got {plan:?}",
    );
}

/// A range pin resolves once; from then on the lockfile names the one pnpm
/// every contributor on the project runs. A running pnpm the range would
/// also have allowed is not that one.
#[test]
fn pre_command_plan_switches_to_the_version_the_pin_resolved_to() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), ">=0.0.0");
    write_lockfile(root.path(), &locked_package_manager(">=0.0.0", "99.0.0"));

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::Switch(plan)) = plan else {
        panic!("expected a switch plan, got {plan:?}");
    };
    let SwitchSource::LockedEnv { version, .. } = plan.target.source else {
        panic!("expected the locked resolution to be switched to");
    };
    assert_eq!(version, "99.0.0");
}

#[test]
fn pre_command_plan_records_a_pin_the_pm_on_fail_setting_reactivated() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        &format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{PNPM_VERSION}","onFail":"ignore"}}}}}}"#,
        ),
    );

    // `pmOnFail` overrides the manifest's own `onFail`, so the pin the
    // manifest asked to ignore is enforced — and recorded — after all.
    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &config_overrides(&["--config.pm-on-fail=warn"]),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(
        matches!(plan, Some(PreCommandPlan::SyncEnvLockfile(_))),
        "expected an env lockfile sync, got {plan:?}",
    );
}

#[test]
fn pre_command_plan_does_not_record_a_pin_the_pm_on_fail_setting_turned_off() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &config_overrides(&["--config.pm-on-fail=ignore"]),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected pre-command plan: {plan:?}");
}

fn config_overrides(argv: &[&str]) -> ConfigOverrides {
    ConfigOverrides::extract(argv.iter().copied().map(OsString::from)).0
}

fn pre_command_input(dir: &Path) -> PreCommandInput {
    PreCommandInput {
        switch: SwitchInput {
            dir: dir.to_path_buf(),
            state_dir: None,
            npmrc_auth_file: None,
            command: Some("run".to_string()),
            frozen_lockfile: None,
            color: None,
        },
        global: false,
        skip_pm_handling: false,
        check_runtimes: true,
        syncs_env_lockfile_in_pipeline: false,
        emit: SilentReporter::emit,
        key_issues: KeyIssueReporting::Enforce,
    }
}

#[test]
fn switch_target_prefers_locked_dev_engine_version() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"^11.0.0-rc.5","onFail":"download"}}}"#,
    );
    write_lockfile(
        root.path(),
        r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 11.1.2
        version: 11.1.2
      pnpm:
        specifier: 11.1.2
        version: 11.1.2

packages:

  '@pnpm/exe@11.1.2':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@11.1.2:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@11.1.2': {}

  pnpm@11.1.2: {}
---
",
    );

    let target =
        switch_target(&Config::default(), root.path(), false).expect("target").expect("switch");

    assert_eq!(target.spec, "^11.0.0-rc.5");
    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "11.1.2");
}

#[test]
fn switch_target_accepts_peer_suffixed_package_manager_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_PEER_SUFFIX);

    let target =
        switch_target(&Config::default(), root.path(), false).expect("target").expect("switch");

    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "9.3.0");
}

#[test]
fn switch_target_accepts_v12_lockfile_without_legacy_wrapper_entry() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@99.0.0"}"#);
    write_lockfile(root.path(), LOCKED_99_0_0);

    let target =
        switch_target(&Config::default(), root.path(), false).expect("target").expect("switch");

    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "99.0.0");
}

#[test]
fn switch_target_discards_package_manager_lockfile_resolution_with_non_integrity_fields() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_TARBALL_RESOLUTION);

    let target =
        switch_target(&Config::default(), root.path(), false).expect("target").expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: true,
        locked_version,
    } = target.source
    else {
        panic!("expected a forced re-resolve, got {:?}", target.source);
    };
    assert_eq!(env_root, root.path());
    assert_eq!(locked_version.as_deref(), Some("9.3.0"));
}

#[test]
fn switch_target_discards_package_manager_lockfile_dependency_with_non_registry_dep_path() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_FILE_DEP_PATH);

    let target =
        switch_target(&Config::default(), root.path(), false).expect("target").expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: true,
        locked_version,
    } = target.source
    else {
        panic!("expected a forced re-resolve, got {:?}", target.source);
    };
    assert_eq!(env_root, root.path());
    assert_eq!(locked_version.as_deref(), Some("9.3.0"));
}

#[test]
fn switch_target_reresolves_when_locked_version_no_longer_satisfies_range() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), ">=9.1.2 <9.1.4");
    write_lockfile(root.path(), LOCKED_9_1_1);

    let target =
        switch_target(&Config::default(), root.path(), false).expect("target").expect("switch");

    assert_eq!(target.spec, ">=9.1.2 <9.1.4");
    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected resolve target");
    };
    assert_eq!(env_root, root.path());
}

#[test]
fn switch_target_uses_global_env_for_legacy_package_manager_field() {
    let root = TempDir::new().expect("tmp dir");
    let global_pkg_dir = root.path().join("pnpm-home").join("global");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config { global_pkg_dir: Some(global_pkg_dir.clone()), ..Config::default() },
        root.path(),
        false,
    )
    .expect("target")
    .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected resolve target");
    };
    assert_eq!(target.spec, "9.3.0");
    assert_eq!(env_root, global_pkg_dir);
}

#[test]
fn switch_target_respects_pm_on_fail_ignore() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config {
            pm_on_fail: Some(PmOnFail::Ignore),
            global_pkg_dir: Some(root.path().join("pnpm-home").join("global")),
            ..Config::default()
        },
        root.path(),
        false,
    )
    .expect("target");

    assert!(target.is_none(), "unexpected switch target: {target:?}");
}

#[test]
fn switch_target_refuses_to_record_a_persisting_pin_under_frozen_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), ">=9.1.2 <9.1.4");
    write_lockfile(root.path(), LOCKED_9_1_1);

    let target =
        switch_target(&Config::default(), root.path(), true).expect("target").expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: true,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected a frozen resolve target, got {:?}", target.source);
    };
    assert_eq!(env_root, root.path());
}

#[test]
fn switch_target_leaves_the_global_env_writable_under_frozen_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    let global_pkg_dir = root.path().join("pnpm-home").join("global");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config { global_pkg_dir: Some(global_pkg_dir.clone()), ..Config::default() },
        root.path(),
        true,
    )
    .expect("target")
    .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected an unfrozen resolve target, got {:?}", target.source);
    };
    assert_eq!(env_root, global_pkg_dir);
}

#[test]
fn switch_target_does_not_switch_dev_engine_without_download() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"9.3.0","onFail":"error"}}}"#,
    );

    let target = switch_target(&Config::default(), root.path(), false).expect("target");

    assert!(target.is_none(), "unexpected switch target: {target:?}");
}

#[test]
fn the_switch_reads_frozen_lockfile_from_the_command_line() {
    let flag_of = |argv: &[&str]| frozen_lockfile_flag(&parse_command(argv));

    assert_eq!(flag_of(&["pnpm", "install", "--frozen-lockfile"]), Some(true));
    assert_eq!(flag_of(&["pnpm", "install", "--no-frozen-lockfile"]), Some(false));
    assert_eq!(flag_of(&["pnpm", "install"]), None);
    assert_eq!(flag_of(&["pnpm", "install-test", "--frozen-lockfile"]), Some(true));
    assert_eq!(flag_of(&["pnpm", "ci"]), Some(true));
    // Only the install family carries the flag; every other command leaves the
    // `frozenLockfile` setting to answer on its own.
    assert_eq!(flag_of(&["pnpm", "run", "build"]), None);
}

fn parse_command(argv: &[&str]) -> CliCommand {
    with_boolean_negations(CliArgs::command())
        .try_get_matches_from(argv)
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
        .expect("parse the command line")
        .command
}

fn write_dev_engine_manifest(root: &Path, version: &str) {
    write_manifest(
        root,
        &format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{version}","onFail":"download"}}}}}}"#,
        ),
    );
}

fn write_manifest(root: &Path, content: &str) {
    fs::write(root.join("package.json"), content).expect("write manifest");
}

fn write_lockfile(root: &Path, content: &str) {
    fs::write(root.join("pnpm-lock.yaml"), content).expect("write lockfile");
}

/// An env lockfile whose `packageManagerDependencies` record `version`,
/// resolved from the registry, against `specifier`.
fn locked_package_manager(specifier: &str, version: &str) -> String {
    format!(
        r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {{}}
    packageManagerDependencies:
      pnpm:
        specifier: '{specifier}'
        version: {version}

packages:

  pnpm@{version}:
    resolution: {{integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}}

snapshots:

  pnpm@{version}: {{}}
---
",
    )
}

const LOCKED_9_3_0_WITH_PEER_SUFFIX: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 9.3.0
        version: 9.3.0
      pnpm:
        specifier: 9.3.0
        version: 9.3.0

packages:

  '@pnpm/exe@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  '@pnpm/linux-x64@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  peer-provider@1.0.0:
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@9.3.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.3.0':
    optionalDependencies:
      '@pnpm/linux-x64': 9.3.0(peer-provider@1.0.0)

  '@pnpm/linux-x64@9.3.0(peer-provider@1.0.0)':
    dependencies:
      peer-provider: 1.0.0
    optional: true

  peer-provider@1.0.0: {}

  pnpm@9.3.0: {}
---
";

const LOCKED_9_3_0_WITH_TARBALL_RESOLUTION: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 9.3.0
        version: 9.3.0
      pnpm:
        specifier: 9.3.0
        version: 9.3.0

packages:

  '@pnpm/exe@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  '@pnpm/linux-x64@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==, tarball: https://evil.example.com/pnpm-linux-x64.tgz}

  pnpm@9.3.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.3.0':
    optionalDependencies:
      '@pnpm/linux-x64': 9.3.0

  '@pnpm/linux-x64@9.3.0':
    optional: true

  pnpm@9.3.0: {}
---
";

const LOCKED_9_3_0_WITH_FILE_DEP_PATH: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 9.3.0
        version: 9.3.0
      pnpm:
        specifier: 9.3.0
        version: 9.3.0

packages:

  '@pnpm/exe@9.3.0':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  payload@file:../payload.tgz:
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@9.3.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.3.0': {}

  payload@file:../payload.tgz: {}

  pnpm@9.3.0:
    dependencies:
      payload: file:../payload.tgz
---
";

const LOCKED_9_1_1: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: '>=9.1.0 <9.1.2'
        version: 9.1.1
      pnpm:
        specifier: '>=9.1.0 <9.1.2'
        version: 9.1.1

packages:

  '@pnpm/exe@9.1.1':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@9.1.1:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@9.1.1': {}

  pnpm@9.1.1: {}
---
";

const LOCKED_99_0_0: &str = r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      pnpm:
        specifier: 99.0.0
        version: 99.0.0

packages:

  pnpm@99.0.0:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  pnpm@99.0.0: {}
---
";
