use super::{
    ConfigOverrides, EntryPoint, Invocation, PinHandling, PreCommandPlan, SwitchProcessState,
    TempDir, UNREADABLE_LOCKFILE_SETTING, pre_command_input, pre_command_plan_from_input,
    write_dev_engine_manifest, write_lockfile, write_manifest, write_workspace_manifest,
};
use pnpm_config::PNPM_VERSION;
use std::{ffi::OsString, path::Path};

const SWITCH_ALLOWED: SwitchProcessState =
    SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false };

fn dlx_input(dir: &Path) -> Invocation {
    Invocation {
        command: Some("dlx".to_string()),
        pin_handling: PinHandling::SwitchOnly,
        ..pre_command_input(dir)
    }
}

fn plan_for_dlx(dir: &Path, process_state: SwitchProcessState) -> Option<PreCommandPlan> {
    pre_command_plan_from_input(&dlx_input(dir), &ConfigOverrides::default(), process_state)
        .expect("pre-command plan")
}

#[test]
fn dlx_only_switches_to_the_pinned_pnpm() {
    let pin_handling_of = |args: &[&str]| {
        let argv: Vec<OsString> = args
            .iter()
            .copied()
            .map(OsString::from)
            .collect();
        Invocation::from_argv(&argv, EntryPoint::Command).pin_handling
    };

    assert_eq!(pin_handling_of(&["pnpm", "dlx", "cowsay"]), PinHandling::SwitchOnly);
    assert_eq!(pin_handling_of(&["pnpm", "store", "path"]), PinHandling::Skip);
    assert_eq!(pin_handling_of(&["pnpm", "install"]), PinHandling::Enforce);
}

/// The pinned pnpm may have added a setting the running one cannot read
/// (zkochan/pnpm-tasks#94).
#[test]
fn dlx_switches_to_the_pinned_pnpm_past_a_setting_this_pnpm_cannot_read() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "99.0.0");
    write_workspace_manifest(root.path(), UNREADABLE_LOCKFILE_SETTING);

    let plan = plan_for_dlx(root.path(), SWITCH_ALLOWED);

    let Some(PreCommandPlan::Switch(plan)) = plan else {
        panic!("expected a switch plan, got {plan:?}");
    };
    assert_eq!(plan.target.spec, "99.0.0");
}

/// `pnpm dlx yarn` is how the error for a yarn project tells the user to run
/// yarn.
#[test]
fn dlx_runs_in_a_project_pinned_to_another_package_manager() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"yarn@4.0.0"}"#);

    assert!(plan_for_dlx(root.path(), SWITCH_ALLOWED).is_none());
}

#[test]
fn dlx_does_not_check_a_pnpm_pin_it_cannot_switch_to() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"0.0.1","onFail":"error"}}}"#,
    );

    assert!(plan_for_dlx(root.path(), SWITCH_ALLOWED).is_none());
    let under_corepack =
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: true };
    assert!(plan_for_dlx(root.path(), under_corepack).is_none());
}

#[test]
fn dlx_does_not_record_the_pin() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);

    assert!(plan_for_dlx(root.path(), SWITCH_ALLOWED).is_none());
    let switch_disabled =
        SwitchProcessState { package_manager_switch_disabled: true, executed_by_corepack: false };
    write_dev_engine_manifest(root.path(), "99.0.0");
    assert!(plan_for_dlx(root.path(), switch_disabled).is_none());
}

#[test]
fn dlx_does_not_check_the_runtime_pin() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"runtime":{"name":"node","version":"99999.0.0","onFail":"error"}}}"#,
    );

    assert!(plan_for_dlx(root.path(), SWITCH_ALLOWED).is_none());
}

#[test]
fn dlx_does_not_read_the_env_lockfile_when_it_cannot_switch() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        &format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{PNPM_VERSION}","onFail":"error"}}}}}}"#,
        ),
    );
    write_lockfile(root.path(), MALFORMED_ENV_DOCUMENT);

    let enforced = pre_command_plan_from_input(
        &pre_command_input(root.path()),
        &ConfigOverrides::default(),
        SWITCH_ALLOWED,
    );
    assert!(enforced.is_err(), "the fixture must fail a command that reads it: {enforced:?}");
    assert!(plan_for_dlx(root.path(), SWITCH_ALLOWED).is_none());
}

#[test]
fn dlx_does_not_read_the_env_lockfile_for_the_running_pnpm() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), PNPM_VERSION);
    write_lockfile(root.path(), MALFORMED_ENV_DOCUMENT);

    assert!(plan_for_dlx(root.path(), SWITCH_ALLOWED).is_none());
}

const MALFORMED_ENV_DOCUMENT: &str = "---\nlockfileVersion: [\n---\n";
