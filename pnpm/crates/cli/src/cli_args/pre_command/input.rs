use super::{
    ArgTable, CliArgs, CliCommand, ColorMode, Config, ConfigLocation, ConfigSubcommand,
    InstallArgs, LockfileDirArg, LogEvent, OsStr, OsString, PACKAGE_MANAGER_SWITCH_ENV_VARS, Path,
    PathBuf, resolve_bool_override,
};

pub(super) struct PreCommandInput {
    pub(super) switch: SwitchInput,
    pub(super) global: bool,
    pub(super) skip_pm_handling: bool,
    pub(super) check_runtimes: bool,
    pub(super) emit: fn(&LogEvent),
    pub(super) key_issues: KeyIssueReporting,
}

/// What to do about the problem keys of the project's `pnpm-workspace.yaml`
/// (see [`report_workspace_key_issues`](crate::cli_args::config_warnings::report_workspace_key_issues)), decided per invocation.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum KeyIssueReporting {
    /// `pnpm config get <key>` prints one value for a script to capture, so
    /// config-load warnings stay off it entirely, as pnpm keeps them off.
    Skip,
    /// The other `pnpm config` subcommands are how a user inspects and
    /// repairs the config, so they must keep working on a broken file:
    /// unrecognized keys warn even under a satisfied pin.
    WarnOnly,
    /// Unrecognized keys warn, or fail the command when the running pnpm is
    /// the version the project pins (no version-skew excuse remains).
    Enforce,
}

pub(super) fn key_issue_reporting(command: &CliCommand) -> KeyIssueReporting {
    match command {
        CliCommand::Get(get) if get.args.key.is_some() => KeyIssueReporting::Skip,
        CliCommand::Config(args) => match &args.command {
            ConfigSubcommand::Get(get) if get.key.is_some() => KeyIssueReporting::Skip,
            _ => KeyIssueReporting::WarnOnly,
        },
        CliCommand::Get(_) | CliCommand::Set(_) => KeyIssueReporting::WarnOnly,
        _ => KeyIssueReporting::Enforce,
    }
}

/// The install-family commands that sync `packageManagerDependencies` from
/// their own pipeline, where the effective `frozen-lockfile` value is known —
/// the commands whose dispatch calls
/// `pipelines::derive_config_root_and_package_manager_to_sync`.
/// See [`env_lockfile_sync`](super::env_lockfile_sync).
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
            CliCommand::Add(args) => Self::of_lockfile_dir(&args.lockfile_dir),
            CliCommand::Ci(args) => Self::of_install(&args.install_args),
            CliCommand::Dedupe(args) => Self {
                lockfile_dir: None,
                offline: typed_flag(args.offline, args.no_offline),
                prefer_offline: typed_flag(args.prefer_offline, args.no_prefer_offline),
            },
            CliCommand::Deploy(args) => Self::of_install(&args.install_args),
            CliCommand::Install(args) => Self::of_install(args),
            CliCommand::InstallTest(args) => Self::of_install(&args.install_args),
            CliCommand::Pipeline(args) => Self::of_install(&args.install_args),
            CliCommand::Remove(args) => Self::of_lockfile_dir(&args.lockfile_dir),
            CliCommand::Update(args) => Self::of_lockfile_dir(&args.lockfile_dir),
            _ => Self::default(),
        }
    }

    fn of_install(args: &InstallArgs) -> Self {
        Self {
            lockfile_dir: args.lockfile_dir.lockfile_dir.clone(),
            offline: typed_flag(args.offline, args.no_offline),
            prefer_offline: typed_flag(args.prefer_offline, args.no_prefer_offline),
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
pub(super) fn is_global(command: &CliCommand) -> bool {
    match command {
        CliCommand::Add(args) => args.global,
        CliCommand::ApproveBuilds(args) => args.global,
        CliCommand::Bin(args) => args.global,
        CliCommand::Config(args) => args.flags.global,
        CliCommand::Get(args) => args.flags.global,
        CliCommand::Set(args) => args.flags.global,
        CliCommand::Env(args) => args.global,
        CliCommand::List(args) | CliCommand::Ll(args) => args.global,
        CliCommand::Outdated(args) => args.global,
        CliCommand::Prefix(args) => args.global,
        CliCommand::Remove(args) => args.global,
        CliCommand::Root(args) => args.global,
        CliCommand::Runtime(args) => args.global,
        CliCommand::Update(args) => args.global,
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
pub(super) fn should_skip_command(command: &CliCommand) -> bool {
    // Declaring which package manager a project uses is not that package
    // manager's work, so a project pinned to another one can still be
    // told to use a different one — or to use this one. Adding anything
    // else stays its manager's job and still fails the check.
    if let CliCommand::Add(args) = command
        && !args.package_names.is_empty()
        && args
            .package_names
            .iter()
            .all(|request| crate::engine_pm::pin::declared_package_manager(request).is_some())
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

pub(super) struct SwitchInput {
    pub(super) dir: PathBuf,
    pub(super) state_dir: Option<PathBuf>,
    pub(super) npmrc_auth_file: Option<PathBuf>,
    pub(super) command: Option<String>,
    /// `--frozen-lockfile` / `--no-frozen-lockfile` as typed on the command
    /// line. `None` leaves the `frozenLockfile` setting to answer.
    pub(super) frozen_lockfile: Option<bool>,
    /// The install-family options the pin record reads.
    pub(super) pin_flags: PinFlags,
    pub(super) color: Option<ColorMode>,
    /// `--ignore-workspace` as typed on the command line. It suppresses
    /// the workspace search for this pass as it does for the install, so
    /// no `pnpm-workspace.yaml` is read at all and the `packageManager`
    /// pin comes from the project's own `package.json`.
    pub(super) ignore_workspace: bool,
}

impl SwitchInput {
    pub(super) fn from_cli_args(args: &CliArgs) -> Self {
        Self {
            dir: args.dir.clone(),
            state_dir: args.state_dir.clone(),
            npmrc_auth_file: args.npmrc_auth_file.clone(),
            command: Some(command_name(&args.command).to_string()),
            frozen_lockfile: frozen_lockfile_flag(&args.command),
            pin_flags: PinFlags::of(&args.command),
            color: args.color.or_else(|| args.no_color.then_some(ColorMode::Never)),
            ignore_workspace: args.ignore_workspace,
        }
    }

    /// The `--dir` a command line without one resolves to.
    ///
    /// Degrades to `.`, which the caller canonicalizes: a cwd that cannot
    /// be resolved fails there, with the path it was given.
    fn local_prefix_or_cwd() -> PathBuf {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| super::super::prefix::find_local_prefix(&cwd).ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(super) fn from_version_argv(argv: &[OsString]) -> Self {
        let global_options = ArgTable::top_level(super::super::grammar());
        let mut input = Self {
            dir: Self::local_prefix_or_cwd(),
            state_dir: None,
            npmrc_auth_file: None,
            command: None,
            frozen_lockfile: None,
            pin_flags: PinFlags::default(),
            color: None,
            ignore_workspace: false,
        };
        let mut index = 1;
        while index < argv.len() {
            let Some(token) = argv[index].to_str() else {
                // A non-UTF-8 token is not a flag this pass knows, and
                // naming no command keeps the caller out of the skip list.
                input.command = Some(String::new());
                break;
            };
            if token == "--" {
                break;
            }
            if !token.starts_with('-') {
                input.command = Some(token.to_string());
                break;
            }
            let next = argv.get(index + 1).map(OsString::as_os_str);
            index += input.absorb_global_flag(token, next, &global_options);
        }
        input
    }

    /// Read one global flag this pass cares about, returning how many
    /// argv tokens it consumed.
    fn absorb_global_flag(
        &mut self,
        token: &str,
        next: Option<&std::ffi::OsStr>,
        global_options: &ArgTable,
    ) -> usize {
        if let Some(value) = short_value(token, "-C", next) {
            self.dir = PathBuf::from(value);
            return if token == "-C" { 2 } else { 1 };
        }
        if let Some((value, width)) =
            long_value(token, "dir", next).or_else(|| long_value(token, "prefix", next))
        {
            self.dir = PathBuf::from(value);
            return width;
        }
        if let Some((value, width)) = long_value(token, "state-dir", next) {
            self.state_dir = Some(PathBuf::from(value));
            return width;
        }
        if let Some(set) = boolean_flag(token, "ignore-workspace") {
            self.ignore_workspace = set;
            return 1;
        }
        if let Some((value, width)) = long_value(token, "npmrc-auth-file", next)
            .or_else(|| long_value(token, "userconfig", next))
        {
            self.npmrc_auth_file = Some(PathBuf::from(value));
            return width;
        }
        if consumes_next_token(token, global_options) { 2 } else { 1 }
    }
}

fn command_name(command: &CliCommand) -> &'static str {
    command.into()
}

fn short_value<'a>(token: &'a str, option: &str, next: Option<&'a OsStr>) -> Option<&'a OsStr> {
    if token == option {
        return next;
    }
    token.strip_prefix(option).filter(|value| !value.is_empty()).map(OsStr::new)
}

/// Read a bare boolean global flag, in the two spellings that reach this
/// scan: `--<option>` and `--<option>=<bool>`, which
/// [`resolve_boolean_values`](crate::boolean_values::resolve_boolean_values)
/// has already folded into `--<option>` or `--no-<option>`.
fn boolean_flag(token: &str, option: &str) -> Option<bool> {
    let name = token.strip_prefix("--")?;
    if name == option {
        return Some(true);
    }
    (name.strip_prefix("no-") == Some(option)).then_some(false)
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
    let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
        return false;
    };
    let short = rest.chars().next().expect("checked non-empty");
    rest.chars().count() == 1 && global_options.short_consumes_value(short).unwrap_or(false)
}
