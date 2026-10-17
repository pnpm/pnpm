pub(super) mod bootstrap_argv;

pub(super) use argv_scan::{UnparsedArgv, VersionArgv};

mod argv_scan;

use super::{
    ArgTable, CliArgs, CliCommand, CliNetworkArgs, ColorMode, Config, ConfigLocation, InstallArgs,
    LockfileDirArg, LogEvent, OsStr, OsString, PACKAGE_MANAGER_SWITCH_ENV_VARS, Path, PathBuf,
    ReporterFlags, pre_command_emit, resolve_bool_override,
};
use crate::cli_args::{
    cli_command::{CliPathArgs, PresentationArgs},
    config_warnings::ConfigWarnings,
    install::InstallLockfileArgs,
    install_options::OfflineArgs,
};

pub(super) struct PreCommandInput {
    pub(super) switch: SwitchInput,
    pub(super) global: bool,
    pub(super) skip_pm_handling: bool,
    pub(super) check_runtimes: bool,
    pub(super) reporter: ReporterFlags,
    /// How the command reports the warnings of its configuration, which
    /// decides whether a configuration it fails on may record the pin.
    pub(super) warnings: ConfigWarnings,
}

impl PreCommandInput {
    pub(super) fn emit(&self, config: &Config) -> fn(&LogEvent) {
        pre_command_emit(self.reporter.configure_with(config))
    }
}

/// `--frozen-lockfile` / `--no-frozen-lockfile` as typed on the command line.
/// Only the install family carries the flags, and `pnpm ci` is a frozen
/// install whether or not they were typed.
pub(super) fn frozen_lockfile_flag(command: &CliCommand) -> Option<bool> {
    match command {
        CliCommand::Install(args) => args.frozen_lockfile_flag(),
        CliCommand::InstallTest(args) => args.install_args.frozen_lockfile_flag(),
        CliCommand::Ci(_) => Some(true),
        _ => None,
    }
}

/// The install-family options the pin record reads, as typed on the command
/// line.
///
/// One list, because a command that grows one of these flags has to be added
/// in one place — `pin_flags_cover_every_command_declaring_them` fails when
/// it is not.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct PinFlags {
    /// `--lockfile-dir`, which moves the lockfile the pin is recorded in.
    pub(super) lockfile_dir: Option<PathBuf>,
    /// `--offline` / `--no-offline`, which decide whether the record may be
    /// resolved over the network.
    pub(super) offline: Option<bool>,
    /// `--prefer-offline` / `--no-prefer-offline`, which decide whether the
    /// resolution reaches for the cache first.
    pub(super) prefer_offline: Option<bool>,
}

impl PinFlags {
    pub(super) fn of(command: &CliCommand) -> Self {
        match command {
            CliCommand::Add(args) => Self::of_lockfile_dir(&args.install.lockfile_dir),
            CliCommand::Ci(args) => Self::of_install(&args.install_args),
            CliCommand::Dedupe(args) => Self::of_offline_flags(&args.network_cache),
            CliCommand::Deploy(args) => Self::of_install(&args.install_args),
            CliCommand::Install(args) => Self::of_install(args),
            CliCommand::InstallTest(args) => Self::of_install(&args.install_args),
            CliCommand::Pipeline(args) => Self::of_install(&args.install_args),
            CliCommand::Remove(args) => Self::of_lockfile_dir(&args.lockfile_dir),
            CliCommand::Update(args) => Self::of_lockfile_dir(&args.install.lockfile_dir),
            _ => Self::default(),
        }
    }

    fn of_install(args: &InstallArgs) -> Self {
        Self::of_install_flags(&args.lockfile, &args.network_cache)
    }

    pub(super) fn of_install_flags(
        lockfile: &InstallLockfileArgs,
        network_cache: &OfflineArgs,
    ) -> Self {
        Self {
            lockfile_dir: lockfile.directory.lockfile_dir.clone(),
            ..Self::of_offline_flags(network_cache)
        }
    }

    fn of_offline_flags(network_cache: &OfflineArgs) -> Self {
        Self {
            lockfile_dir: None,
            offline: typed_flag(network_cache.offline, network_cache.no_offline),
            prefer_offline: typed_flag(
                network_cache.prefer_offline,
                network_cache.no_prefer_offline,
            ),
        }
    }

    fn of_lockfile_dir(lockfile_dir: &LockfileDirArg) -> Self {
        Self { lockfile_dir: lockfile_dir.lockfile_dir.clone(), ..Self::default() }
    }

    /// Layer the flags onto `config` with the precedence
    /// [`resolve_bool_override`] gives a `--flag` / `--no-flag` pair, so
    /// `--no-offline` clears a configured `offline` here exactly as it does
    /// for an install.
    pub(super) fn apply_to(&self, config: &mut Config, dir: &Path) {
        if let Some(lockfile_dir) = self.lockfile_dir.as_deref() {
            config.pin_lockfile_dir(&dir.join(lockfile_dir));
        }
        config.offline = resolve_bool_override(
            self.offline == Some(true),
            self.offline == Some(false),
            config.offline,
        );
        config.prefer_offline = resolve_bool_override(
            self.prefer_offline == Some(true),
            self.prefer_offline == Some(false),
            config.prefer_offline,
        );
    }
}

/// A `--flag` / `--no-flag` pair as typed on the command line, or `None`
/// when neither spelling was.
fn typed_flag(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

/// pnpm treats `--global` as an opt-out of the project's package manager
/// and runtime pins — a global install does not belong to the project.
pub(crate) fn is_global(command: &CliCommand) -> bool {
    match command {
        CliCommand::Add(args) => args.target.global,
        CliCommand::ApproveBuilds(args) => args.global,
        CliCommand::Bin(args) => args.global,
        CliCommand::Config(args) => args.flags.is_global(),
        CliCommand::Get(args) => args.flags.is_global(),
        CliCommand::Set(args) => args.flags.is_global(),
        CliCommand::Env(args) => args.global,
        CliCommand::List(args) | CliCommand::Ll(args) => args.global,
        CliCommand::Outdated(args) => args.global,
        CliCommand::Prefix(args) => args.global,
        CliCommand::Remove(args) => args.global,
        CliCommand::Root(args) => args.global,
        CliCommand::Runtime(args) => args.global,
        CliCommand::Update(args) => args.selection.global,
        // `pnpm link` with no arguments links the current project into the
        // global directory.
        CliCommand::Link(args) => args.package_paths.is_empty(),
        _ => false,
    }
}

/// The commands pnpm marks with `skipPackageManagerCheck`, plus `setup` —
/// they either predate the project's pins (`setup`), inspect pnpm itself
/// (`store`, `doctor`, and so on), or run something that is not the project
/// (`dlx`).
pub(crate) fn should_skip_command(command: &CliCommand) -> bool {
    // Declaring which package manager a project uses is not that package
    // manager's work, so a project pinned to another one can still be
    // told to use a different one — or to use this one. Adding anything
    // else stays its manager's job and still fails the check.
    if let CliCommand::Add(args) = command
        && !args.package_names.is_empty()
        && args.package_names
            .iter()
            .all(|request| {
                crate::engine_pm::pin::declared_package_manager(request.selector()).is_some()
            })
    {
        return true;
    }
    matches!(
        command,
        CliCommand::CatFile(_)
            | CliCommand::CatIndex(_)
            | CliCommand::Completion(_)
            | CliCommand::CompletionServer(_)
            | CliCommand::Dlx(_)
            | CliCommand::Doctor(_)
            | CliCommand::FindHash(_)
            | CliCommand::Runtime(_)
            | CliCommand::Env(_)
            | CliCommand::Edit(_)
            | CliCommand::Profile(_)
            | CliCommand::Token(_)
            | CliCommand::Xmas(_)
            | CliCommand::SelfUpdate(_)
            | CliCommand::Setup(_)
            | CliCommand::Shim(_)
            | CliCommand::Store(_)
            | CliCommand::With(_),
    )
}

pub(super) fn should_skip_pm_handling(command: &CliCommand) -> bool {
    match command {
        CliCommand::Config(args) => args.flags.location != Some(ConfigLocation::Project),
        CliCommand::Get(args) => args.flags.location != Some(ConfigLocation::Project),
        CliCommand::Set(args) => args.flags.location != Some(ConfigLocation::Project),
        _ => false,
    }
}

pub(super) fn should_skip_command_name(command: &str) -> bool {
    matches!(
        command,
        "cat-file"
            | "cat-index"
            | "completion"
            | "completion-server"
            | "dlx"
            | "doctor"
            | "edit"
            | "env"
            | "find-hash"
            | "profile"
            | "runtime"
            | "rt"
            | "self-update"
            | "setup"
            | "shim"
            | "store"
            | "token"
            | "with"
            | "xmas",
    )
}

pub(super) fn package_manager_switch_disabled() -> bool {
    PACKAGE_MANAGER_SWITCH_ENV_VARS.into_iter().any(env_var_is_false)
}

fn env_var_is_false(name: &str) -> bool {
    std::env::var_os(name)
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "false" | "0"))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SwitchInput {
    /// `--ignore-workspace` suppresses the workspace search for this pass
    /// as it does for the install, so no `pnpm-workspace.yaml` is read at
    /// all and the `packageManager` pin comes from the project's own
    /// manifest.
    pub(super) paths: CliPathArgs,
    pub(super) command: Option<String>,
    /// `--frozen-lockfile` / `--no-frozen-lockfile` as typed on the command
    /// line. `None` leaves the `frozenLockfile` setting to answer.
    pub(super) frozen_lockfile: Option<bool>,
    /// The install-family options the pin record reads.
    pub(super) pin_flags: PinFlags,
    pub(super) color: Option<ColorMode>,
    /// The registry and proxy flags. The pinned pnpm is resolved and
    /// downloaded through them, as every other package is.
    pub(super) network: CliNetworkArgs,
}

impl SwitchInput {
    pub(super) fn from_cli_args(args: &CliArgs) -> Self {
        Self::from_flags(SwitchFlags {
            command: Some(command_name(&args.command).to_string()),
            paths: args.paths.clone(),
            network: args.network.clone(),
            presentation: &args.output.presentation,
            frozen_lockfile: frozen_lockfile_flag(&args.command),
            pin_flags: PinFlags::of(&args.command),
        })
    }

    /// The one constructor every way of reading the command line ends in,
    /// so each reads the same clap groups the same way.
    fn from_flags(flags: SwitchFlags<'_>) -> Self {
        let SwitchFlags {
            command,
            paths,
            network,
            presentation,
            frozen_lockfile,
            pin_flags,
        } = flags;
        Self {
            paths,
            command,
            frozen_lockfile,
            pin_flags,
            color: presentation.color.or_else(|| presentation.no_color.then_some(ColorMode::Never)),
            network,
        }
    }
}

/// What [`SwitchInput::from_flags`] reads.
struct SwitchFlags<'a> {
    command: Option<String>,
    paths: CliPathArgs,
    network: CliNetworkArgs,
    presentation: &'a PresentationArgs,
    frozen_lockfile: Option<bool>,
    pin_flags: PinFlags,
}

fn command_name(command: &CliCommand) -> &'static str {
    command.into()
}

fn long_value<'a>(
    token: &'a str,
    option: &str,
    next: Option<&'a OsStr>,
) -> Option<(&'a OsStr, usize)> {
    let name = token.strip_prefix("--")?;
    if name == option {
        return next.map(|value| (value, 2));
    }
    name.strip_prefix(option)
        .and_then(|rest| rest.strip_prefix('='))
        .map(OsStr::new)
        .map(|value| (value, 1))
}

/// Whether an option token takes the following argv token as its value, so
/// the scan steps over it instead of mistaking it for the command name.
/// Read from the clap grammar rather than a list of names, so an option (or
/// alias) added to [`CliArgs`] is accounted for here without a second edit.
fn consumes_next_token(token: &str, global_options: &ArgTable) -> bool {
    if let Some(name) = token.strip_prefix("--") {
        return !name.contains('=') && global_options.long_consumes_value(name).unwrap_or(false);
    }
    let Some(rest) = token
        .strip_prefix('-')
        .filter(|rest| !rest.is_empty())
    else {
        return false;
    };
    let short = rest
        .chars()
        .next()
        .expect("checked non-empty");
    rest.chars().count() == 1 && global_options.short_consumes_value(short).unwrap_or(false)
}
