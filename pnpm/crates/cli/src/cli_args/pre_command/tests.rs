use super::{
    CliNetworkArgs, ConfigLoad, EntryPoint, Invocation, PackageManagerToSync, PinInput, PinRoots,
    PreCommandPlan, SwitchProcessState, SwitchSource, SwitchTarget, apply_cli_layers,
    load_pre_command_config, locked_package_manager_to_fetch, pin::switch_target_from_pin,
    pre_command_plan_with_state, read_env_lockfile, read_root_manifest, wanted_package_manager,
};
use crate::{
    cli_args::{
        cli_command::CliPathArgs,
        config_warnings::ConfigWarnings,
        reporter::{ReporterFlags, ReporterType},
    },
    config_overrides::ConfigOverrides,
};
use pnpm_config::{Config, PNPM_VERSION, PmOnFail};
use std::{ffi::OsString, fs, path::Path};
use tempfile::TempDir;

fn pre_command_plan_from_input(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    process_state: SwitchProcessState,
) -> miette::Result<Option<PreCommandPlan>> {
    Ok(pre_command_plan_with_state(invocation, config_overrides, process_state)?.plan)
}

fn switch_target(
    config: &Config,
    roots: &PinRoots,
    frozen_lockfile: bool,
) -> miette::Result<Option<SwitchTarget>> {
    let manifest = read_root_manifest(&roots.manifest);
    let wanted = manifest.as_ref().and_then(wanted_package_manager);
    let locked_env = if config.lockfile { read_env_lockfile(&roots.env)? } else { None };
    let pin = PinInput {
        roots,
        manifest: manifest.as_ref(),
        wanted: wanted.as_ref(),
        locked_env,
        running_version: PNPM_VERSION,
        process_state: SwitchProcessState {
            package_manager_switch_disabled: false,
            executed_by_corepack: false,
        },
    };
    switch_target_from_pin(&pin, config, frozen_lockfile)
}

#[test]
fn the_warning_carries_the_code_and_help_of_a_diagnostic() {
    #[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
    #[display("the engine could not be installed")]
    #[diagnostic(code(ERR_PNPM_TEST), help("Pin an exact version."))]
    struct Failed;

    let warning = super::warning_for_unusable_pin(&miette::Report::new(Failed));

    assert!(warning.contains("ERR_PNPM_TEST"), "{warning}");
    assert!(warning.contains("the engine could not be installed"), "{warning}");
    assert!(warning.contains("Pin an exact version."), "{warning}");
}

#[test]
fn the_reported_causes_redact_registry_credentials() {
    let error = miette::miette!("fetch https://user:hunter2@registry.example.com/pnpm failed");

    let causes = super::error_causes(&error);

    assert!(!causes.contains("hunter2"), "credentials reached the warning: {causes}");
    assert!(causes.contains("registry.example.com"), "the host should survive: {causes}");
}

#[test]
fn the_pre_command_config_resolves_the_store_dir_flag() {
    let root = TempDir::new().expect("tmp dir");
    let dir = dunce::canonicalize(root.path()).expect("canonicalize the project directory");
    write_workspace_manifest(&dir, "");
    let mut input = version_switch_input(&[
        OsString::from("pnpm"),
        OsString::from("--store-dir"),
        OsString::from("relative-store"),
        OsString::from("--version"),
    ]);
    input.paths.dir = dir.clone();

    let config =
        load_pre_command_config(&input, &ConfigOverrides::default(), &dir, ConfigLoad::SWITCH)
            .expect("load the pre-command config");

    assert_eq!(
        config.store_dir.root(),
        dir.join("relative-store").join(pnpm_store_dir::STORE_VERSION),
    );
}

#[test]
fn the_pre_command_config_resolves_the_pinned_pnpm_through_the_network_flags() {
    let root = TempDir::new().expect("tmp dir");
    let dir = dunce::canonicalize(root.path()).expect("canonicalize the project directory");
    let mut input = version_switch_input(&[
        OsString::from("pnpm"),
        OsString::from("--registry=https://mirror.example.com/npm"),
        OsString::from("--https-proxy"),
        OsString::from("http://proxy.example.com:8080"),
        OsString::from("--version"),
    ]);
    input.paths.dir = dir.clone();

    let config =
        load_pre_command_config(&input, &ConfigOverrides::default(), &dir, ConfigLoad::SWITCH)
            .expect("load the pre-command config");

    assert_eq!(config.package_manager_bootstrap.registry, "https://mirror.example.com/npm/");
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://mirror.example.com/npm/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.proxy.https_proxy.as_deref(),
        Some("http://proxy.example.com:8080"),
    );
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
fn pre_command_plan_yields_no_plan_when_root_manifest_is_not_an_object() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), "null");

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
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
        &Invocation { global: true, ..pre_command_input(root.path()) },
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
            running_pnpm_for_range: false,
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

/// The install family records the pin here like every other command, so
/// there is one writer and no command list to keep in step with it.
#[test]
fn pre_command_plan_records_the_pin_for_the_install_family_too() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    for command in ["install", "add", "ci", "update", "remove", "dedupe", "prune", "unlink"] {
        let mut input = pre_command_input(root.path());
        input.command = Some(command.to_string());
        let plan = pre_command_plan_from_input(
            &input,
            &ConfigOverrides::default(),
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("pre-command plan");

        assert!(
            matches!(plan, Some(PreCommandPlan::SyncEnvLockfile(_))),
            "expected an env lockfile sync for {command}, got {plan:?}",
        );
    }
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
fn pre_command_plan_skips_the_env_lockfile_sync_for_a_compatible_extra_entry() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);
    write_lockfile(root.path(), &locked_package_manager_with_exe(PNPM_VERSION, PNPM_VERSION));

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

/// A range pin the lockfile already resolved keeps that version, even while
/// the running pnpm is another one the range allows. Recording the running
/// pnpm instead would pin everyone to a release nobody checked against the
/// project's `minimumReleaseAge` (pnpm/pnpm#16431).
#[test]
fn pre_command_plan_keeps_the_locked_version_of_a_range_pin() {
    let (range, locked) = range_around_the_running_pnpm();
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), &range);
    write_lockfile(root.path(), &locked_package_manager(&range, &locked));

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected pre-command plan: {plan:?}");
}

#[test]
fn pre_command_plan_rerecords_the_locked_version_under_a_changed_range() {
    let (range, locked) = range_around_the_running_pnpm();
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), &range);
    write_lockfile(root.path(), &locked_package_manager(">=0.0.0", &locked));

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::SyncEnvLockfile(sync)) = plan else {
        panic!("expected an env lockfile sync, got {plan:?}");
    };
    assert_eq!(
        sync.package_manager,
        PackageManagerToSync { specifier: range, version: locked, running_pnpm_for_range: false },
    );
}

#[test]
fn pre_command_plan_flags_the_running_pnpm_recorded_for_a_range_pin() {
    let (range, _) = range_around_the_running_pnpm();
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), &range);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::SyncEnvLockfile(sync)) = plan else {
        panic!("expected an env lockfile sync, got {plan:?}");
    };
    assert_eq!(
        sync.package_manager,
        PackageManagerToSync {
            specifier: range,
            version: PNPM_VERSION.to_string(),
            running_pnpm_for_range: true,
        },
    );
}

/// A caret range over the running pnpm's major, which the source checkout's
/// pnpm (a different major) cannot satisfy, and another version inside it.
fn range_around_the_running_pnpm() -> (String, String) {
    let running = node_semver::Version::parse(PNPM_VERSION).expect("parse the running version");
    let range = format!("^{}.0.0", running.major);
    let other = format!("{}.999.0", running.major);
    assert_ne!(other, PNPM_VERSION);
    (range, other)
}

/// The install family records the pin from its own pipeline whether or not
/// version switching is on, so every other command has to record it there
/// too — otherwise the two rewrite each other forever (pnpm/pnpm#14575).
#[test]
fn pre_command_plan_records_a_pin_when_version_switching_is_turned_off() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::SyncEnvLockfile(sync)) = plan else {
        panic!("expected an env lockfile sync, got {plan:?}");
    };
    assert_eq!(
        sync.package_manager,
        PackageManagerToSync {
            specifier: PNPM_VERSION.to_string(),
            version: PNPM_VERSION.to_string(),
            running_pnpm_for_range: false,
        },
    );
}

/// A pin the running pnpm cannot satisfy resolves to no version to record,
/// so turning the switch off never cements one nobody runs.
#[test]
fn pre_command_plan_records_nothing_for_an_unsatisfiable_pin_when_switching_is_turned_off() {
    for spec in ["^999.0.0", "^11.0.0"] {
        let root = TempDir::new().expect("tmp dir");
        write_dev_engine_manifest(root.path(), spec);

        let plan = pre_command_plan_from_input(
            &pre_command_input(root.path()),
            &ConfigOverrides::default(),
            SwitchProcessState {
                package_manager_switch_disabled: true,
                executed_by_corepack: false,
            },
        )
        .expect("pre-command plan");

        assert!(plan.is_none(), "unexpected pre-command plan for {spec}: {plan:?}");
    }
}

#[test]
fn pre_command_plan_records_nothing_for_a_global_command_when_switching_is_turned_off() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    let plan = pre_command_plan_from_input(
        &Invocation { global: true, ..pre_command_input(root.path()) },
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected pre-command plan: {plan:?}");
}

#[test]
fn pre_command_plan_does_not_switch_a_global_command_to_the_pinned_pnpm() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "0.0.1");

    let plan = pre_command_plan_from_input(
        &Invocation { global: true, ..pre_command_input(root.path()) },
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(plan.is_none(), "unexpected pre-command plan: {plan:?}");
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

/// `lockfile: false` must suppress the project env-lockfile write even when
/// `devEngines.packageManager.onFail: download` would otherwise persist the
/// pin (pnpm/pnpm#14728). Download switching itself stays available.
#[test]
fn pre_command_plan_skips_env_lockfile_sync_when_lockfile_is_disabled() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &config_overrides(&["--no-lockfile"]),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    assert!(
        plan.is_none(),
        "lockfile: false must not schedule a project env lockfile sync, got {plan:?}",
    );
}

/// `lockfile: false` silences the pin record, not the switch: a pin the
/// running pnpm cannot satisfy is still downloaded, and resolves outside the
/// project because the project has no lockfile to resolve into
/// (pnpm/pnpm#14728).
#[test]
fn pre_command_plan_still_switches_when_lockfile_is_disabled() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "99.0.0");

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &config_overrides(&["--no-lockfile"]),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::Switch(plan)) = plan else {
        panic!("expected a switch plan, got {plan:?}");
    };
    assert_eq!(plan.target.spec, "99.0.0");
    let SwitchSource::Resolve { env_root, .. } = &plan.target.source else {
        panic!("expected a resolve target, got {:?}", plan.target.source);
    };
    assert_ne!(env_root.as_path(), root.path());
}

/// A setting this pnpm cannot read may be one the pinned pnpm reads, so it
/// does not stop the switch (pnpm/pnpm#16675).
#[test]
fn pre_command_plan_switches_past_a_setting_this_pnpm_cannot_read() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "99.0.0");
    write_workspace_manifest(root.path(), UNREADABLE_LOCKFILE_SETTING);

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::Switch(plan)) = plan else {
        panic!("expected a switch plan, got {plan:?}");
    };
    assert_eq!(plan.target.spec, "99.0.0");
}

#[test]
fn pre_command_plan_switches_past_a_setting_this_pnpm_rejects() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "99.0.0");
    write_workspace_manifest(root.path(), "tasks:\n  build:\n    concurrency: 0\n");

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::Switch(plan)) = plan else {
        panic!("expected a switch plan, got {plan:?}");
    };
    assert_eq!(plan.target.spec, "99.0.0");
}

/// The decision reads the pin decision settings alone, so a setting outside
/// them is left to the command's own load to report, unreadable or not.
#[test]
fn pre_command_plan_decides_past_a_setting_outside_the_decision() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "99.0.0");
    write_workspace_manifest(
        root.path(),
        &format!("pmOnFail: warn\nnodeLinker: [hoisted]\n{UNREADABLE_LOCKFILE_SETTING}"),
    );

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("the unreadable settings are not the decision's to report");

    assert!(
        !matches!(plan, Some(PreCommandPlan::Switch(_))),
        "pmOnFail: warn keeps the running pnpm: {plan:?}",
    );
}

/// A manifest that pins nothing these checks act on needs no setting, so a
/// workspace file that is not even YAML does not reach them.
#[test]
fn pre_command_plan_reads_no_setting_for_a_manifest_without_pins() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"name":"project"}"#);
    write_workspace_manifest(root.path(), "packages: [\n");

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("no setting is read");

    assert!(plan.is_none(), "{plan:?}");
}

/// A workspace package follows the pin at the workspace root, which the
/// decision finds through the workspace's `packages`.
#[test]
fn pre_command_plan_switches_a_workspace_package_to_the_root_pin() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "99.0.0");
    write_workspace_manifest(root.path(), "packages:\n  - packages/*\n");
    let package_dir = root.path().join("packages/app");
    fs::create_dir_all(&package_dir).expect("create the workspace package");
    write_manifest(&package_dir, r#"{"name":"app"}"#);

    let plan = pre_command_plan_from_input(
        &pre_command_input(&package_dir),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("pre-command plan");

    let Some(PreCommandPlan::Switch(plan)) = plan else {
        panic!("expected a switch plan, got {plan:?}");
    };
    assert_eq!(plan.target.spec, "99.0.0");
}

/// Recording the pin writes the lockfile, so a configuration the command
/// cannot run with records nothing and is left to the command to report.
#[test]
fn pre_command_plan_records_no_pin_under_an_unreadable_setting() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);
    write_workspace_manifest(root.path(), "nodeLinker: [hoisted]\n");

    let plan = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    )
    .expect("the unreadable setting is the command's to report");

    assert!(plan.is_none(), "{plan:?}");
}

const UNREADABLE_LOCKFILE_SETTING: &str = "lockfile:\n  recordsSomethingNew: true\n";

/// `pnpm fetch` has only the lockfile to go on, so it installs the pnpm the
/// env document records whenever a command in the project would switch to
/// it (pnpm/pnpm#11808).
#[test]
fn fetch_picks_the_pnpm_the_lockfile_pins_when_a_command_would_switch_to_it() {
    let switching =
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false };
    let root = TempDir::new().expect("tmp dir");
    let locked = |config: &Config, process_state| {
        locked_package_manager_to_fetch(config, root.path(), process_state)
            .expect("read the env lockfile")
            .map(|(_, version)| version)
    };

    assert_eq!(locked(&Config::default(), switching), None, "no lockfile");

    write_lockfile(root.path(), &locked_package_manager("99.0.0", "99.0.0"));
    assert_eq!(locked(&Config::default(), switching).as_deref(), Some("99.0.0"));
    assert_eq!(
        locked(
            &Config::default(),
            SwitchProcessState {
                package_manager_switch_disabled: true,
                executed_by_corepack: false
            },
        ),
        None,
        "version switching turned off",
    );
    assert_eq!(
        locked(
            &Config::default(),
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: true
            },
        ),
        None,
        "run by corepack",
    );
    for on_fail in [PmOnFail::Ignore, PmOnFail::Warn, PmOnFail::Error] {
        let config = Config { pm_on_fail: Some(on_fail), ..Config::default() };
        assert_eq!(locked(&config, switching), None, "pmOnFail {on_fail:?}");
    }

    write_lockfile(root.path(), &locked_package_manager(PNPM_VERSION, PNPM_VERSION));
    assert_eq!(locked(&Config::default(), switching), None, "the running pnpm");
}

fn pin_roots(dir: &Path) -> PinRoots {
    PinRoots { manifest: dir.to_path_buf(), env: dir.to_path_buf() }
}

fn config_overrides(argv: &[&str]) -> ConfigOverrides {
    ConfigOverrides::extract(argv.iter().copied().map(OsString::from)).0
}

fn pre_command_input(dir: &Path) -> Invocation {
    Invocation {
        entry_point: EntryPoint::Command,
        paths: CliPathArgs {
            dir: dir.to_path_buf(),
            dir_from_command_line: true,
            store_dir: None,
            state_dir: None,
            npmrc_auth_file: None,
            ignore_workspace: false,
            workspace_packages: Vec::new(),
        },
        command: Some("run".to_string()),
        frozen_lockfile: None,
        lockfile_dir: None,
        offline: None,
        prefer_offline: None,
        color: None,
        no_color: false,
        network: CliNetworkArgs::default(),
        global: false,
        project_location: false,
        skip_pm_handling: false,
        check_runtimes: true,
        reporter: ReporterFlags { reporter: Some(ReporterType::Silent), loglevel: None },
        warnings: ConfigWarnings::for_version_flag(),
        ambiguous: false,
    }
}

fn write_dev_engine_manifest(root: &Path, version: &str) {
    write_manifest(
        root,
        &format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{version}","onFail":"download"}}}}}}"#,
        ),
    );
}

fn write_workspace_manifest(root: &Path, content: &str) {
    fs::write(root.join("pnpm-workspace.yaml"), content).expect("write pnpm-workspace.yaml");
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

fn locked_package_manager_with_exe(specifier: &str, version: &str) -> String {
    format!(
        r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {{}}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: '{specifier}'
        version: {version}
      pnpm:
        specifier: '{specifier}'
        version: {version}

packages:

  '@pnpm/exe@{version}':
    resolution: {{integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}}

  pnpm@{version}:
    resolution: {{integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}}

snapshots:

  '@pnpm/exe@{version}': {{}}

  pnpm@{version}: {{}}
---
",
    )
}

mod argv_readers;
mod pin_decision_matrix;
mod switch_target;

fn unparsed_switch_input(argv: &[OsString]) -> Option<Invocation> {
    let inv = Invocation::from_argv(argv, EntryPoint::UnknownFlag);
    (!inv.ambiguous).then_some(inv)
}

fn version_switch_input(argv: &[OsString]) -> Invocation {
    Invocation::from_argv(argv, EntryPoint::Version)
}
