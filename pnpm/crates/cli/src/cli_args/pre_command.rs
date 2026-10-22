//! pnpm's pre-command handling: before a command runs, the pinned package
//! manager and the pinned runtimes are reconciled with what is actually
//! running. A pnpm pin with `onFail: "download"` switches the CLI to the
//! wanted version; every other pin is validated in place and fails or warns.
//! A pin that persists to the lockfile is also recorded there, whatever the
//! command — see [`env_lockfile_sync`].
//!
//! Mirrors the block at the top of pnpm's `pnpm/src/main.ts`.

pub(crate) use argv_plans::{pre_command_plan_for_version_flag, switch_plan_for_unparsed_argv};
pub(crate) use execute::execute_plan;
pub(crate) use input::{is_global, should_skip_command};

mod system_runtime_version;

use super::{
    cli_command::{CliArgs, CliCommand, CliNetworkArgs},
    config::ConfigLocation,
    install::{InstallArgs, resolve_bool_override},
    lockfile_dir::LockfileDirArg,
    package_manager::{
        PACKAGE_MANAGER_SWITCH_ENV_VARS, PackageManagerToSync, PinRoots, WantedPackageManager,
        package_manager_to_sync, read_root_manifest, should_persist_package_manager_lockfile,
        version_satisfies, wanted_package_manager,
    },
    reporter::{ReporterFlags, ReporterType, configure_color, reporter_emit},
    sanitize::sanitize_inline,
    self_update::install_pnpm::{assert_release_is_installable, pnpm_package_to_install},
    with::{PackageManagerCheck, spawn_pnpm},
};
use crate::{
    cli_args::{
        config_warnings::{ConfigWarnings, emit_config_warning},
        dispatch::seed_config,
    },
    config_deps,
    config_overrides::{ConfigOverrides, apply_state_dir_override, apply_store_dir_override},
    engine_pm::{
        channel::PackageManager,
        install::{InstalledEngine, install_engine_from_env, install_engine_to_store},
    },
    flag_relocation::ArgTable,
};
use derive_more::{Display, Error};

use input::{
    PreCommandInput, SwitchInput, package_manager_switch_disabled, should_skip_command_name,
    should_skip_pm_handling,
};
use load_config::{ConfigLoad, load_pre_command_config};
use lockfile::{
    ReadEnvLockfile, env_lockfile_sync, env_lockfile_sync_plan, locked_package_manager_to_fetch,
    locked_package_manager_version, locked_switch_source, read_env_lockfile, switch_env_root,
};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pin::{PinOutcome, PinResolution, resolve_input_pin, switch_target};
use pnpm_config::{ColorMode, Config, Host, PNPM_VERSION, PmOnFail};
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

/// Run the pre-command checks and return the work they ask for, if any. The
/// checks themselves report through `args.reporter` and fail the command by
/// returning an error.
pub(crate) fn pre_command_plan(
    args: &CliArgs,
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<PreCommandPlan>> {
    if should_skip_command(&args.command) {
        return Ok(None);
    }
    pre_command_plan_from_input(
        &PreCommandInput {
            switch: SwitchInput::from_cli_args(args),
            global: is_global(&args.command),
            skip_pm_handling: should_skip_pm_handling(&args.command),
            check_runtimes: true,
            reporter: args.reporter_flags(),
            warnings: ConfigWarnings::of(&args.command),
        },
        config_overrides,
        SwitchProcessState::current(),
    )
}

/// The settings are read in tiers, each only when the one before it calls
/// for it: none when the manifest pins nothing these checks act on, the
/// [`PIN_DECISION_SETTINGS`](pnpm_config::PIN_DECISION_SETTINGS) to decide,
/// and every readable setting to switch or to record the pin. Validating the
/// configuration and reporting its warnings is left to the command, which
/// loads all of it anyway.
fn pre_command_plan_from_input(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    process_state: SwitchProcessState,
) -> miette::Result<Option<PreCommandPlan>> {
    if input.switch.command.as_deref().is_some_and(should_skip_command_name) {
        return Ok(None);
    }
    let dir = canonicalize_dir(&input.switch.paths.dir)?;
    if pins_nothing_to_check(input, &dir) {
        return Ok(None);
    }
    let config = load_pre_command_config(input, config_overrides, &dir, ConfigLoad::PIN_DECISION)?;
    let pin = resolve_pin(input, config_overrides, &dir, process_state, config)?;
    let (config, package_manager_to_sync) = match pin.action {
        PreCommandAction::Switch(plan) => return Ok(Some(PreCommandPlan::Switch(plan))),
        PreCommandAction::Continue { config, package_manager_to_sync } => {
            (config, package_manager_to_sync)
        }
    };
    check_manifest_runtimes(input, &config, pin.manifest)?;
    Ok(package_manager_to_sync.map(|package_manager| {
        env_lockfile_sync_plan(input, config, pin.env_root, package_manager)
    }))
}

/// Whether no manifest the pin could be read from pins pnpm or a runtime
/// these checks act on, so no setting can change what they do.
///
/// [`PinRoots`] reads the pin from the workspace root, or from `dir` outside
/// a workspace. Telling which without loading the configuration would take
/// the workspace's `packages`, so every candidate is checked instead: `dir`,
/// the workspace directory an environment variable names, and the directory
/// of the nearest `pnpm-workspace.yaml`.
fn pins_nothing_to_check(input: &PreCommandInput, dir: &Path) -> bool {
    if input.skip_pm_handling {
        return true;
    }
    let mut candidates = vec![dir.to_path_buf()];
    if !input.switch.paths.ignore_workspace {
        candidates.extend(pnpm_workspace::find_workspace_dir_from_env());
        candidates.push(pnpm_config::workspace_root_or(dir));
    }
    candidates.sort();
    candidates.dedup();
    let checks_runtimes = input.check_runtimes && !input.global;
    candidates
        .iter()
        .filter_map(|candidate| read_root_manifest(candidate))
        .all(|manifest| {
            wanted_package_manager(&manifest).is_none()
                && !(checks_runtimes && declares_runtime(&manifest))
        })
}

/// What the project's pin asks of this invocation, and what was read to
/// decide it.
struct PinReading {
    action: PreCommandAction,
    manifest: Option<Value>,
    env_root: PathBuf,
}

fn resolve_pin(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    process_state: SwitchProcessState,
    config: Config,
) -> miette::Result<PinReading> {
    let roots = PinRoots::of(&config, dir);
    let manifest = read_root_manifest(&roots.manifest);

    let wanted_pm = manifest.as_ref().and_then(wanted_package_manager);
    let outcome =
        resolve_input_pin(input, &config, &roots, process_state, manifest.as_ref(), wanted_pm)?;
    let action = plan_pin_action(outcome, input, config_overrides, dir, config)?;
    Ok(PinReading { action, manifest, env_root: roots.env })
}

fn canonicalize_dir(path: &Path) -> miette::Result<PathBuf> {
    dunce::canonicalize(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("canonicalizing the `--dir` argument: {}", path.display()))
}

enum PreCommandAction {
    Switch(SwitchPlan),
    Continue { config: Config, package_manager_to_sync: Option<PackageManagerToSync> },
}

/// A switch reads every setting this pnpm can read, since the pinned pnpm
/// may read what this one cannot. Recording the pin writes the lockfile, so
/// it waits for a configuration the command can run with: one it cannot is
/// left to the command to report, with nothing recorded.
fn plan_pin_action(
    outcome: PinOutcome,
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    config: Config,
) -> miette::Result<PreCommandAction> {
    match outcome {
        PinOutcome::Switch(target) => {
            let mut config =
                load_pre_command_config(input, config_overrides, dir, ConfigLoad::SWITCH)?;
            // A global command does not act on the project. Without a
            // workspace, the approvals go to the project, as a regular
            // install's do.
            if !input.global {
                config.target_workspace_dir =
                    Some(config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf()));
            }
            Ok(PreCommandAction::Switch(SwitchPlan { config, target }))
        }
        PinOutcome::Sync(Some(sync)) => {
            let record = load_pre_command_config(input, config_overrides, dir, ConfigLoad::RECORD)
                .ok()
                .filter(|config| !input.warnings.fail(config, dir));
            Ok(match record {
                Some(config) => {
                    PreCommandAction::Continue { config, package_manager_to_sync: Some(sync) }
                }
                None => PreCommandAction::Continue { config, package_manager_to_sync: None },
            })
        }
        PinOutcome::Sync(None) => {
            Ok(PreCommandAction::Continue { config, package_manager_to_sync: None })
        }
    }
}

fn check_manifest_runtimes(
    input: &PreCommandInput,
    config: &Config,
    manifest: Option<Value>,
) -> miette::Result<()> {
    if input.check_runtimes
        && !input.skip_pm_handling
        && !input.global
        && let Some(manifest) = manifest
    {
        check_runtimes(manifest, config, input.emit(config))?;
    }
    Ok(())
}

/// Switch to the pinned pnpm, unless the running one already is it — in
/// which case the pin still has to reach the lockfile, which the switch
/// would otherwise have written on its way to the wanted version.
fn switch_or_sync(
    resolution: &PinResolution<'_>,
    root_manifest: &Value,
    on_fail: PmOnFail,
) -> miette::Result<PinOutcome> {
    let PinResolution { config, roots, switch, .. } = *resolution;
    let frozen_lockfile = switch.frozen_lockfile.or(config.frozen_lockfile).unwrap_or(false);
    let Some(target) = switch_target(config, roots, frozen_lockfile)? else {
        return Ok(PinOutcome::Sync(None));
    };
    if target.switches_away_from_the_running_pnpm() {
        return Ok(PinOutcome::Switch(target));
    }
    let read_lockfile = match &target.source {
        SwitchSource::LockedEnv { env, .. } => ReadEnvLockfile::Already(env),
        SwitchSource::Resolve { .. } => ReadEnvLockfile::NotYet,
    };
    Ok(PinOutcome::Sync(env_lockfile_sync(config, root_manifest, roots, on_fail, read_lockfile)?))
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
///
/// The command succeeds, so this is a warning rather than a diagnostic
/// miette renders. It carries the code and the help a diagnostic came
/// with, which is what that rendering would have added.
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

/// Every cause of `error`, in miette's order, dropping the ones an earlier
/// cause already quotes — a wrapping error usually renders its source. A
/// fetch that failed quotes the registry URL it was given, which carries
/// the credentials configured for that registry, so each cause is redacted
/// on its way to the terminal.
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
struct SwitchProcessState {
    package_manager_switch_disabled: bool,
    executed_by_corepack: bool,
}

impl SwitchProcessState {
    fn current() -> Self {
        Self {
            package_manager_switch_disabled: package_manager_switch_disabled(),
            executed_by_corepack: std::env::var_os("COREPACK_ROOT").is_some(),
        }
    }
}

#[derive(Debug)]
struct SwitchTarget {
    spec: String,
    source: SwitchSource,
}

impl SwitchTarget {
    /// Whether reaching the pinned pnpm means running another one.
    ///
    /// A resolution recorded in the env lockfile is the pnpm the project
    /// runs, whatever else its specifier would have allowed, so that
    /// resolution — not the specifier — answers this. Only without one does
    /// the specifier decide: the switch resolves it, and a running pnpm the
    /// specifier already accepts is spared the round trip. A recorded
    /// resolution that has to be repaired is always carried out, because the
    /// repairing write happens there.
    fn switches_away_from_the_running_pnpm(&self) -> bool {
        match &self.source {
            SwitchSource::LockedEnv { version, .. } => version != PNPM_VERSION,
            SwitchSource::Resolve { locked_version, .. } => {
                locked_version.is_some() || !version_satisfies(PNPM_VERSION, &self.spec)
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
enum SwitchSource {
    LockedEnv {
        env: EnvLockfile,
        version: String,
    },
    Resolve {
        env_root: PathBuf,
        /// Refuse to record the resolution instead of writing it. Only set
        /// when `env_root` is the project itself: a global env lockfile is
        /// not what `--frozen-lockfile` freezes.
        frozen_lockfile: bool,
        /// Discard the recorded `packageManagerDependencies` and re-resolve
        /// them even when they look up to date — set when the recorded
        /// entries failed the bootstrap validation, so the resync heals the
        /// env lockfile instead of no-op'ing on the invalid entries.
        force_resync: bool,
        /// The version those invalid entries record. A frozen lockfile
        /// cannot record a fresh pick, so the repair re-resolves this
        /// version rather than the range around it — the switch then runs
        /// the pnpm the lockfile pins, which is what the flag is for.
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

mod argv_plans;

mod load_config;
