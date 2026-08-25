use super::{CommandTestExt, is_pnpm_config_var};
use crate::env_guard::EnvGuard;
use command_extra::CommandExtra;
use std::{ffi::OsStr, process::Command};

const SETTING: &str = "PNPM_CONFIG_GLOBAL_SHIMS";
const CI_SETTING: &str = "PNPM_CONFIG_CI";

#[test]
fn pnpm_and_npm_settings_match_in_either_spelling() {
    for name in [
        SETTING,
        "pnpm_config_global_shims",
        "PnPm_CoNfIg_GlObAl_ShImS",
        "NPM_CONFIG_REGISTRY",
        "npm_config_registry",
        "PNPM_SHIM_BYPASS",
        "pnpm_shim_bypass",
    ] {
        assert!(is_pnpm_config_var(name), "{name} should be stripped");
    }
}

/// The suites locate the ambient pnpm they spawn for compatibility checks
/// through the environment, and a bare prefix names no setting at all. The
/// last name puts a multi-byte character across the prefix boundary, which
/// a slice would panic on rather than reject.
#[test]
fn the_environment_the_suites_rely_on_is_left_alone() {
    for name in [
        "PNPM_HOME",
        "PATH",
        "HOME",
        "PNPM_CONFIG_",
        "npm_config_",
        "PNPM_SHIM_BYPASS_X",
        "aaaaaaaaaaaéz",
    ] {
        assert!(!is_pnpm_config_var(name), "{name} should be kept");
    }
}

/// The removal happens at construction precisely so that a test wanting one
/// of these settings can still set it — the documented contract callers
/// depend on.
#[test]
fn an_explicit_value_set_afterwards_survives_the_removal() {
    let guard = EnvGuard::snapshot([SETTING]);
    guard.set(SETTING, r#"{"node":false}"#);

    let stripped = Command::new("pnpm").without_ambient_pnpm_config();
    assert_eq!(env_value(&stripped, SETTING), None, "the inherited value should be removed");

    let overridden =
        Command::new("pnpm").without_ambient_pnpm_config().with_env(SETTING, r#"{"node":"auto"}"#);
    assert_eq!(
        env_value(&overridden, SETTING).as_deref(),
        Some(OsStr::new(r#"{"node":"auto"}"#)),
        "a later `env` should win over the removal",
    );
}

#[test]
fn spawned_pnpm_defaults_to_non_ci_after_ambient_config_is_removed() {
    let guard = EnvGuard::snapshot([CI_SETTING]);
    guard.set(CI_SETTING, "true");

    let command = Command::new("pnpm").without_ambient_pnpm_config();

    assert_eq!(env_value(&command, CI_SETTING).as_deref(), Some(OsStr::new("false")));
}

/// What `command` will pass for `name`: `None` once it is removed, `Some`
/// once it is set again.
fn env_value(command: &Command, name: &str) -> Option<std::ffi::OsString> {
    command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new(name))
        .and_then(|(_, value)| value)
        .map(OsStr::to_os_string)
}
