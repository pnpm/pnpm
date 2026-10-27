//! pnpm's pre-command handling: before a command runs, the pinned package
//! manager and the pinned runtimes are reconciled with what is actually
//! running. A pnpm pin with `onFail: "download"` switches the CLI to the
//! wanted version; every other pin is validated in place and fails or warns.
//! A pin that persists to the lockfile is also recorded there, whatever the
//! command — see [`env_lockfile_sync_from_loaded`].
//!
//! Mirrors the block at the top of pnpm's `pnpm/src/main.ts`.

pub(crate) use execute::{PlanExecution, execute_plan, execute_plan_reusing_config};
pub(crate) use input::{EntryPoint, Invocation, is_global, should_skip_command};
pub(in crate::cli_args) use load_config::{apply_cli_layers, seed_config};
pub(crate) use pin::{PinDecision, PinInput, decide};

mod system_runtime_version;

use super::{
    cli_command::{CliCommand, CliNetworkArgs},
    package_manager::{
        PACKAGE_MANAGER_SWITCH_ENV_VARS, PackageManagerToSync, PinRoots, WantedPackageManager,
        package_manager_to_sync_for_version, read_root_manifest,
        should_persist_package_manager_lockfile, version_satisfies, wanted_package_manager,
    },
    reporter::{ReporterFlags, ReporterType, reporter_emit},
    sanitize::sanitize_inline,
    self_update::install_pnpm::{assert_release_is_installable, pnpm_package_to_install},
    with::{PackageManagerCheck, spawn_pnpm},
};
use crate::{
    cli_args::config_warnings::emit_config_warning,
    config_deps,
    config_overrides::ConfigOverrides,
    engine_pm::{
        channel::PackageManager,
        install::{InstalledEngine, install_engine_from_env, install_engine_to_store},
    },
    flag_relocation::ArgTable,
};
use derive_more::{Display, Error};

use input::should_skip_command_name;
use load_config::{ConfigLoad, load_pre_command_config, try_load_record_config};
use lockfile::{
    env_lockfile_sync_from_loaded, env_lockfile_sync_plan, locked_package_manager_to_fetch,
    locked_package_manager_version, locked_switch_source, read_env_lockfile, switch_env_root,
};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pin::package_manager_switch_disabled;
use pnpm_config::{ColorMode, Config, ConfigLoadFailure, Host, PNPM_VERSION, PmOnFail};
use pnpm_default_reporter::MaxLogLevel;
use pnpm_env_installer::is_package_manager_resolved;
use pnpm_lockfile::{EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, VersionPart};
use pnpm_network::redact_and_sanitize;
use pnpm_package_manifest::{apply_runtime_on_fail_override, is_runtime_alias};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use runtime::{RUNTIME_ON_FAIL_HINT, check_runtimes, declares_runtime};
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    slice,
};
use system_runtime_version::system_runtime_version;

pub(crate) struct PreCommandOutcome {
    pub(crate) plan: Option<PreCommandPlan>,
    pub(crate) preloaded_config: Option<Result<Config, ConfigLoadFailure>>,
}

/// Run the pre-command checks for `invocation` and return the work they ask
/// for, if any, along with any full [`Config`] loaded along the way.
pub(crate) fn pre_command_plan(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
) -> miette::Result<PreCommandOutcome> {
    pre_command_plan_with_state(invocation, config_overrides, SwitchProcessState::current())
}

/// The `pnpm --version` path, which clap answers before a command is
/// parsed. pnpm checks the package manager there too, but skips the runtime
/// checks — printing the version must work in a project whose runtime pin
/// the system cannot satisfy.
pub(crate) fn pre_command_plan_for_version_flag(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<PreCommandPlan>> {
    let invocation = Invocation::from_argv(argv, EntryPoint::Version);
    Ok(pre_command_plan(&invocation, config_overrides)?.plan)
}

/// A command line clap rejected for an argument this pnpm does not know.
/// The pnpm the project pins may know it, so the command goes to that pnpm
/// when the pin calls for a switch. Anything short of a switch leaves the
/// rejection to stand, and records nothing for a command that is not run.
pub(crate) fn switch_plan_for_unparsed_argv(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<PreCommandPlan>> {
    let invocation = Invocation::from_argv(argv, EntryPoint::UnknownFlag);
    Ok(pre_command_plan(&invocation, config_overrides)?.plan)
}

/// The settings are read in tiers, each only when the one before it calls
/// for it: none when the manifest pins nothing these checks act on, the
/// [`PIN_DECISION_SETTINGS`](pnpm_config::PIN_DECISION_SETTINGS) to decide,
/// and every readable setting to switch or to record the pin. Validating the
/// configuration and reporting its warnings is left to the command, which
/// reuses the loaded configuration.
fn pre_command_plan_with_state(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    process_state: SwitchProcessState,
) -> miette::Result<PreCommandOutcome> {
    if invocation.skip_pm_handling
        || (invocation.entry_point != EntryPoint::SelfUpdate
            && invocation.command.as_deref().is_some_and(should_skip_command_name))
    {
        return Ok(PreCommandOutcome { plan: None, preloaded_config: None });
    }
    let dir = canonicalize_dir(&invocation.paths.dir)?;
    if pins_nothing_to_check(invocation, &dir) {
        return Ok(PreCommandOutcome { plan: None, preloaded_config: None });
    }
    let config =
        load_pre_command_config(invocation, config_overrides, &dir, ConfigLoad::PIN_DECISION)?;
    let roots = PinRoots::of(&config, &dir);
    let manifest = read_root_manifest(&roots.manifest);
    let wanted_pm = manifest.as_ref().and_then(wanted_package_manager);
    let locked_env =
        if wanted_pm.is_some() && config.lockfile { read_env_lockfile(&roots.env)? } else { None };
    let pin_input = PinInput {
        roots: &roots,
        manifest: manifest.as_ref(),
        wanted: wanted_pm.as_ref(),
        locked_env,
        running_version: PNPM_VERSION,
        process_state,
    };
    let decision = decide(&pin_input, &config, invocation)?;
    execute_pin_decision(decision, invocation, config_overrides, &dir, &config, manifest)
}

fn execute_pin_decision(
    decision: PinDecision,
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    config: &Config,
    manifest: Option<Value>,
) -> miette::Result<PreCommandOutcome> {
    match decision {
        PinDecision::Switch(target) => {
            let mut config =
                load_pre_command_config(invocation, config_overrides, dir, ConfigLoad::SWITCH)?;
            if !invocation.global {
                config.target_workspace_dir =
                    Some(config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf()));
            }
            Ok(PreCommandOutcome {
                plan: Some(PreCommandPlan::Switch(SwitchPlan { config, target })),
                preloaded_config: None,
            })
        }
        PinDecision::Record { at: _, package_manager } => {
            check_manifest_runtimes(invocation, config, manifest)?;
            let record_result = try_load_record_config(invocation, config_overrides, dir)?;
            match record_result {
                Ok(record_config) if !invocation.warnings.fail(&record_config, dir) => {
                    let env_root = record_config.root_project_manifest_dir(dir).to_path_buf();
                    Ok(PreCommandOutcome {
                        plan: Some(env_lockfile_sync_plan(
                            invocation,
                            record_config,
                            env_root,
                            package_manager,
                        )),
                        preloaded_config: None,
                    })
                }
                other => Ok(PreCommandOutcome { plan: None, preloaded_config: Some(other) }),
            }
        }
        PinDecision::Nothing => {
            check_manifest_runtimes(invocation, config, manifest)?;
            Ok(PreCommandOutcome { plan: None, preloaded_config: None })
        }
    }
}

/// Whether no manifest the pin could be read from pins pnpm or a runtime
/// these checks act on, so no setting can change what they do.
fn pins_nothing_to_check(invocation: &Invocation, dir: &Path) -> bool {
    if invocation.skip_pm_handling {
        return true;
    }
    let mut candidates = vec![dir.to_path_buf()];
    if !invocation.paths.ignore_workspace {
        candidates.extend(pnpm_workspace::find_workspace_dir_from_env());
        candidates.push(pnpm_config::workspace_root_or(dir));
    }
    candidates.sort();
    candidates.dedup();
    let checks_runtimes = invocation.check_runtimes && !invocation.global;
    candidates
        .iter()
        .filter_map(|candidate| read_root_manifest(candidate))
        .all(|manifest| {
            wanted_package_manager(&manifest).is_none()
                && !(checks_runtimes && declares_runtime(&manifest))
        })
}

fn canonicalize_dir(path: &Path) -> miette::Result<PathBuf> {
    dunce::canonicalize(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("canonicalizing the `--dir` argument: {}", path.display()))
}

fn check_manifest_runtimes(
    invocation: &Invocation,
    config: &Config,
    manifest: Option<Value>,
) -> miette::Result<()> {
    if invocation.check_runtimes
        && !invocation.skip_pm_handling
        && !invocation.global
        && let Some(manifest) = manifest
    {
        check_runtimes(manifest, config, invocation.emit(config))?;
    }
    Ok(())
}

/// Install the pnpm that the env lockfile in `env_root` pins into the store,
/// when a command in the project would switch to it. `pnpm fetch` reads only
/// the lockfile, and a later `pnpm install --offline` that switches to the
/// pinned pnpm finds it there instead of in the registry (pnpm/pnpm#11808).
pub(crate) async fn fetch_locked_package_manager<Reporter: pnpm_reporter::Reporter + 'static>(
    config: &'static Config,
    env_root: &Path,
) -> miette::Result<()> {
    let Some((env, version)) =
        locked_package_manager_to_fetch(config, env_root, SwitchProcessState::current())?
    else {
        return Ok(());
    };
    assert_release_is_installable(&version)?;
    let engine =
        Box::pin(install_engine_from_env::<Reporter>(config, PackageManager::Pnpm, &env, &version))
            .await
            .wrap_err_with(|| format!("fetch pnpm v{version}, which the lockfile pins"))?;
    if engine.private_install.is_some() {
        miette::bail!(
            "could not install pnpm v{version} into the shared store because another process held the install lock",
        );
    }
    Ok(())
}

/// Every warning here quotes the project's manifest, which is untrusted
/// input in a repository the user has only cloned, so control characters
/// are stripped before the message reaches the terminal.
fn global_warn(emit: fn(&LogEvent), message: &str) {
    let message = sanitize_inline(message).into_owned();
    emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}

/// The sink for the warnings the checks here raise. They are not the
/// command's output, so the default reporter's go to stderr: a command such
/// as `pnpm cache path` or `pnpm list --json` prints a value a script reads
/// from stdout.
fn pre_command_emit(reporter: ReporterType) -> fn(&LogEvent) {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => emit_warning_on_stderr,
        ReporterType::Ndjson | ReporterType::Silent => reporter_emit(reporter),
    }
}

/// The default reporter's rendering of a [`global_warn`] event, on stderr.
fn emit_warning_on_stderr(event: &LogEvent) {
    let LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }) = event else {
        unreachable!("the pre-command checks emit only global warnings");
    };
    if pnpm_default_reporter::max_log_level() >= MaxLogLevel::Warn {
        emit_config_warning(message);
    }
}

/// Report a pinned pnpm that `pnpm --version` could not act on. Why the
/// command carries on afterwards is documented on its caller in `lib.rs`.
pub(crate) fn warn_pinned_pnpm_unusable(error: &miette::Report) {
    global_warn(emit_warning_on_stderr, &warning_for_unusable_pin(error));
}

fn warning_for_unusable_pin(error: &miette::Report) -> String {
    let code = error
        .code()
        .map(|code| format!("{code}: "))
        .unwrap_or_default();
    let help = error
        .help()
        .map(|help| format!(". {}", redact_and_sanitize(&help.to_string())))
        .unwrap_or_default();
    format!("Cannot use the pnpm version this project pins: {code}{}{help}", error_causes(error))
}

fn error_causes(error: &miette::Report) -> String {
    let mut causes = String::new();
    for cause in error.chain() {
        let cause = redact_and_sanitize(&cause.to_string());
        if causes.contains(cause.as_str()) {
            continue;
        }
        if !causes.is_empty() {
            causes.push_str(": ");
        }
        causes.push_str(&cause);
    }
    causes
}

#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum PreCommandError {
    #[display("This project is configured to use {name}")]
    #[diagnostic(code(ERR_PNPM_OTHER_PM_EXPECTED), help("{hint}"))]
    OtherPmExpected { name: String, hint: String },

    #[display(
        "This project is configured to use {wanted} of pnpm. Your current pnpm is v{PNPM_VERSION}{note}"
    )]
    #[diagnostic(code(ERR_PNPM_BAD_PM_VERSION), help("{hint}"))]
    BadPmVersion { wanted: String, note: &'static str, hint: String },

    #[diagnostic(code(ERR_PNPM_BAD_RUNTIME_VERSION), help("{RUNTIME_ON_FAIL_HINT}"))]
    BadRuntimeVersion { message: String },
}

#[derive(Clone, Copy)]
pub(crate) struct SwitchProcessState {
    pub(crate) package_manager_switch_disabled: bool,
    pub(crate) executed_by_corepack: bool,
}

impl SwitchProcessState {
    pub(crate) fn current() -> Self {
        Self {
            package_manager_switch_disabled: package_manager_switch_disabled(),
            executed_by_corepack: std::env::var_os("COREPACK_ROOT").is_some(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SwitchTarget {
    pub(crate) spec: String,
    pub(crate) source: SwitchSource,
}

impl SwitchTarget {
    /// Whether reaching the pinned pnpm means running another one than
    /// `running_version`.
    pub(crate) fn switches_away_from(&self, running_version: &str) -> bool {
        match &self.source {
            SwitchSource::LockedEnv { version, .. } => version != running_version,
            SwitchSource::Resolve { locked_version, .. } => {
                locked_version.is_some() || !version_satisfies(running_version, &self.spec)
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct SwitchPlan {
    config: Config,
    target: SwitchTarget,
}

/// The asynchronous work the (synchronous) pre-command checks scheduled.
#[derive(Debug)]
pub(crate) enum PreCommandPlan {
    Switch(SwitchPlan),
    SyncEnvLockfile(EnvLockfileSync),
}

#[derive(Debug)]
pub(crate) struct EnvLockfileSync {
    config: Config,
    /// Where the env lockfile is written: the lockfile's directory, which
    /// `--lockfile-dir` and the `lockfileDir` setting move away from the
    /// workspace root.
    env_root: PathBuf,
    package_manager: PackageManagerToSync,
    frozen_lockfile: bool,
}

#[derive(Debug)]
pub(crate) enum SwitchSource {
    LockedEnv {
        env: EnvLockfile,
        version: String,
    },
    Resolve {
        env_root: PathBuf,
        frozen_lockfile: bool,
        force_resync: bool,
        locked_version: Option<String>,
    },
}

#[cfg(test)]
mod tests;

mod input;

mod lockfile;

mod runtime;

mod pin;

mod execute;

mod load_config;
