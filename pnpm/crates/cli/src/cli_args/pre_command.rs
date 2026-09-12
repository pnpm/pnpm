//! pnpm's pre-command handling: before a command runs, the pinned package
//! manager and the pinned runtimes are reconciled with what is actually
//! running. A pnpm pin with `onFail: "download"` switches the CLI to the
//! wanted version; every other pin is validated in place and fails or warns.
//! A pin that persists to the lockfile is also recorded there, whatever the
//! command — see [`env_lockfile_sync`].
//!
//! Mirrors the block at the top of pnpm's `pnpm/src/main.ts`.

pub(crate) use execute::execute_plan;

mod system_runtime_version;

use super::{
    cli_command::{CliArgs, CliCommand},
    config::{ConfigLocation, ConfigSubcommand},
    install::{InstallArgs, resolve_bool_override},
    lockfile_dir::LockfileDirArg,
    package_manager::{
        PACKAGE_MANAGER_SWITCH_ENV_VARS, PackageManagerToSync, WantedPackageManager,
        package_manager_to_sync, read_manifest_json, should_persist_package_manager_lockfile,
        version_satisfies, wanted_package_manager,
    },
    reporter::reporter_emit,
    sanitize::sanitize_inline,
    self_update::install_pnpm::{assert_release_is_installable, pnpm_package_to_install},
    with::{PackageManagerCheck, spawn_pnpm},
};
use crate::{
    cli_args::{config_warnings::report_workspace_key_issues, dispatch::seed_config},
    config_deps,
    config_overrides::{ConfigOverrides, apply_state_dir_override},
    engine_pm::{
        channel::PackageManager,
        install::{install_engine_from_env, install_engine_to_store},
    },
    flag_relocation::ArgTable,
};
use derive_more::{Display, Error};

use input::{
    KeyIssueReporting, PreCommandInput, SwitchInput, is_global, key_issue_reporting,
    package_manager_switch_disabled, should_skip_command, should_skip_command_name,
    should_skip_pm_handling,
};
use lockfile::{
    ReadEnvLockfile, env_lockfile_sync, env_lockfile_sync_plan, locked_package_manager_version,
    locked_switch_source, read_env_lockfile, switch_env_root,
};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pin::{PinOutcome, PinResolution, resolve_input_pin, switch_target};
use pnpm_config::{ColorMode, Config, Host, PNPM_VERSION, PmOnFail};
use pnpm_default_reporter::DefaultReporter;
use pnpm_env_installer::is_package_manager_resolved;
use pnpm_lockfile::{EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, VersionPart};
use pnpm_package_manifest::{apply_runtime_on_fail_override, is_runtime_alias};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter, SilentReporter};
use runtime::{RUNTIME_ON_FAIL_HINT, check_runtimes};
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
            emit: reporter_emit(args.reporter),
            key_issues: key_issue_reporting(&args.command),
        },
        config_overrides,
        SwitchProcessState::current(),
    )
}

/// The `pnpm --version` path, which clap answers before a command is
/// parsed. pnpm checks the package manager there too, but skips the runtime
/// checks — printing the version must work in a project whose runtime pin
/// the system cannot satisfy.
pub(crate) fn pre_command_plan_for_version_flag(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<PreCommandPlan>> {
    pre_command_plan_from_input(
        &PreCommandInput {
            switch: SwitchInput::from_version_argv(argv),
            global: false,
            skip_pm_handling: false,
            check_runtimes: false,
            emit: DefaultReporter::emit,
            // Printing the version must work in a project whose
            // `pnpm-workspace.yaml` is broken, like the runtime checks above.
            key_issues: KeyIssueReporting::WarnOnly,
        },
        config_overrides,
        SwitchProcessState::current(),
    )
}

fn pre_command_plan_from_input(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    process_state: SwitchProcessState,
) -> miette::Result<Option<PreCommandPlan>> {
    if input.switch.command.as_deref().is_some_and(should_skip_command_name) {
        return Ok(None);
    }
    let dir = dunce::canonicalize(&input.switch.dir).into_diagnostic().wrap_err_with(|| {
        format!("canonicalizing the `--dir` argument: {}", input.switch.dir.display())
    })?;
    let config = load_pre_command_config(&input.switch, config_overrides, &dir)?;

    let roots = PinRoots {
        manifest: config.workspace_dir.clone().unwrap_or_else(|| dir.clone()),
        env: config.root_project_manifest_dir(&dir).to_path_buf(),
    };
    let manifest = read_manifest_json(&roots.manifest.join("package.json"))?;

    let wanted_pm = manifest.as_ref().and_then(wanted_package_manager);
    let running_matches_pin = pin_matches_running(wanted_pm.as_ref());
    let package_manager_to_sync = match resolve_input_pin(
        input,
        &config,
        &roots,
        process_state,
        manifest.as_ref(),
        wanted_pm,
    )? {
        PinOutcome::Switch(target) => {
            return Ok(Some(PreCommandPlan::Switch(SwitchPlan { config, target })));
        }
        PinOutcome::Sync(sync) => sync,
    };

    report_key_issues(input, &config, running_matches_pin)?;

    if input.check_runtimes
        && !input.skip_pm_handling
        && !input.global
        && let Some(manifest) = manifest
    {
        check_runtimes(manifest, &config, input.emit)?;
    }
    Ok(package_manager_to_sync
        .map(|package_manager| env_lockfile_sync_plan(input, config, roots.env, package_manager)))
}

/// Whether the manifest's pin names the pnpm that is running.
fn pin_matches_running(wanted_pm: Option<&WantedPackageManager>) -> bool {
    wanted_pm.is_some_and(|pm| {
        pm.name == "pnpm"
            && pm.version.as_deref().is_some_and(|version| version_satisfies(PNPM_VERSION, version))
    })
}

/// A `--global` invocation does not act on the project, so a satisfied
/// pin does not harden its unrecognized-key report into an error.
fn report_key_issues(
    input: &PreCommandInput,
    config: &Config,
    running_matches_pin: bool,
) -> miette::Result<()> {
    if input.key_issues == KeyIssueReporting::Skip {
        return Ok(());
    }
    let strict =
        input.key_issues == KeyIssueReporting::Enforce && running_matches_pin && !input.global;
    report_workspace_key_issues(&config.workspace_key_issues, strict)?;
    Ok(())
}

/// Load the configuration the pre-command pass reads, with the global
/// CLI flags that reach it applied.
fn load_pre_command_config(
    switch: &SwitchInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
) -> miette::Result<Config> {
    let mut config = seed_config(switch.npmrc_auth_file.as_deref(), switch.ignore_workspace)
        .current::<Host>(dir)
        .map_err(miette::Report::new)
        .wrap_err("load configuration")?;
    config_overrides.apply(&mut config, dir);
    if let Some(color) = switch.color {
        config.color = color;
    }
    super::reporter::configure_color(config.color);
    if config.ci {
        pnpm_default_reporter::force_append_only();
    }
    if let Some(state_dir) = switch.state_dir.as_deref() {
        apply_state_dir_override::<Host>(&mut config, state_dir, dir);
    }
    // `--lockfile-dir` moves the lockfile the pin is recorded in, and
    // `--offline` governs how that record is resolved. Both are
    // install-family flags, and the record below is made for every
    // command.
    switch.pin_flags.apply_to(&mut config, dir);
    Ok(config)
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

/// Every warning here quotes the project's manifest, which is untrusted
/// input in a repository the user has only cloned, so control characters
/// are stripped before the message reaches the terminal.
fn global_warn(emit: fn(&LogEvent), message: &str) {
    let message = sanitize_inline(message).into_owned();
    emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
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

/// The two directories the package-manager pin is read from and written to.
///
/// They differ when `lockfileDir` moves `pnpm-lock.yaml` off the workspace
/// root: the manifest declaring the pin stays at the workspace root, while
/// the env lockfile is the first document of the lockfile and follows it.
#[derive(Debug, Clone)]
struct PinRoots {
    manifest: PathBuf,
    env: PathBuf,
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
