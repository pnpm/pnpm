pub(super) mod bootstrap_argv;

use super::{
    ArgTable, CliCommand, CliNetworkArgs, ColorMode, Config, LogEvent, OsStr, OsString, PathBuf,
    ReporterFlags, pre_command_emit,
};
use crate::{
    boolean_negations::with_boolean_negations,
    cli_args::{
        CliArgs,
        cli_command::{CliPathArgs, PresentationArgs, options::find_workspace_root_dir},
        config_warnings::ConfigWarnings,
    },
};
use bootstrap_argv::{
    BootstrapTokens, canonical_command_name, cluster_options, command_options, consumes_next_token,
    declared_option_width, every_option, long_value, may_take_undeclared_value, typed_flag,
};
use clap::CommandFactory;

/// Which CLI entry point built an [`Invocation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryPoint {
    /// Normal command execution (`pnpm install`, `pnpm run`, etc.).
    Command,
    /// Top-level `pnpm --version` / `pnpm -v`.
    Version,
    /// An invocation clap rejected with `UnknownArgument`.
    UnknownFlag,
    /// `pnpm self-update` syncing the lockfile after updating the manifest pin.
    SelfUpdate,
}

/// One parsed CLI invocation shared by every entry point: pre-command pin
/// handling, `--version`, unknown-flag handoff, `self-update`, and full command
/// config loading.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "Invocation is the single parsed CLI representation shared across every pre-command and dispatch entry point"
    )
)]
pub(crate) struct Invocation {
    pub(crate) entry_point: EntryPoint,
    /// `--ignore-workspace` suppresses the workspace search for this pass
    /// as it does for the install, so no `pnpm-workspace.yaml` is read at
    /// all and the `packageManager` pin comes from the project's own
    /// manifest.
    pub(crate) paths: CliPathArgs,
    pub(crate) command: Option<String>,
    /// `--frozen-lockfile` / `--no-frozen-lockfile` as typed on the command
    /// line. `None` leaves the `frozenLockfile` setting to answer.
    pub(crate) frozen_lockfile: Option<bool>,
    /// `--lockfile-dir`, which moves the lockfile the pin is recorded in.
    pub(crate) lockfile_dir: Option<PathBuf>,
    /// `--offline` / `--no-offline`, which decide whether the record may be
    /// resolved over the network.
    pub(crate) offline: Option<bool>,
    /// `--prefer-offline` / `--no-prefer-offline`, which decide whether the
    /// resolution reaches for the cache first.
    pub(crate) prefer_offline: Option<bool>,
    pub(crate) color: Option<ColorMode>,
    pub(crate) no_color: bool,
    /// The registry and proxy flags. The pinned pnpm is resolved and
    /// downloaded through them, as every other package is.
    pub(crate) network: CliNetworkArgs,
    pub(crate) global: bool,
    pub(crate) project_location: bool,
    pub(crate) skip_pm_handling: bool,
    pub(crate) check_runtimes: bool,
    pub(crate) reporter: ReporterFlags,
    /// How the command reports the warnings of its configuration, which
    /// decides whether a configuration it fails on may record the pin.
    pub(crate) warnings: ConfigWarnings,
    /// True when an undeclared option before the command name or before a
    /// scope flag left the command or scope ambiguous.
    pub(crate) ambiguous: bool,
}

impl Invocation {
    /// Parse `argv` once for `entry_point`.
    pub(crate) fn from_argv(argv: &[OsString], entry_point: EntryPoint) -> Self {
        InvocationScan::scan(argv, entry_point).into_invocation(entry_point)
    }

    /// Build the invocation representing `self-update`'s post-manifest-update
    /// lockfile sync step.
    pub(crate) fn for_self_update(dir: PathBuf) -> Self {
        let mut inv = Self::from_argv(&[], EntryPoint::SelfUpdate);
        inv.paths.dir = dir;
        inv.paths.dir_from_command_line = true;
        inv.command = Some("self-update".to_string());
        inv.warnings = ConfigWarnings::for_unparsed_argv();
        inv
    }

    pub(super) fn emit(&self, config: &Config) -> fn(&LogEvent) {
        pre_command_emit(self.reporter.configure_with(config))
    }
}

/// The state of [`Invocation::from_argv`] as it walks the command line.
#[derive(Default)]
struct InvocationScan {
    command: Option<String>,
    positionals: Vec<String>,
    tokens: BootstrapTokens,
    global: bool,
    project_location: bool,
    workspace_root: bool,
    after_undeclared_option: bool,
    ambiguous: bool,
}

impl InvocationScan {
    fn scan(argv: &[OsString], entry_point: EntryPoint) -> Self {
        let cli = with_boolean_negations(CliArgs::command());
        let mut options = every_option(&cli);
        let mut scan = Self::default();
        let mut index = 1;
        while index < argv.len() {
            let Some(token) = argv[index].to_str() else {
                scan.mark_non_utf8(entry_point);
                break;
            };
            if token == "--" {
                break;
            }
            let next = argv
                .get(index + 1)
                .map(OsString::as_os_str);
            let named_command = scan.command.is_some();
            let next_is_command = next
                .and_then(OsStr::to_str)
                .is_some_and(|next| cli.find_subcommand(next).is_some());
            let Some(width) = scan.absorb(token, next, &options, next_is_command, entry_point)
            else {
                scan.ambiguous = true;
                break;
            };
            index += width;
            if scan.update_command_options(&cli, &mut options, named_command, entry_point) {
                break;
            }
        }
        scan
    }

    fn mark_non_utf8(&mut self, entry_point: EntryPoint) {
        if entry_point == EntryPoint::UnknownFlag {
            self.ambiguous = true;
        } else if entry_point == EntryPoint::Version {
            self.command.get_or_insert_with(String::new);
        }
    }

    fn update_command_options(
        &self,
        cli: &clap::Command,
        options: &mut ArgTable,
        named_command: bool,
        entry_point: EntryPoint,
    ) -> bool {
        if let Some(command) = self.command.as_deref().filter(|_| !named_command) {
            *options = command_options(cli, command);
            if matches!(command, "test" | "start" | "stop")
                || (entry_point != EntryPoint::UnknownFlag
                    && cli.find_subcommand(command).is_none())
            {
                return true;
            }
        }
        matches!(self.command.as_deref(), Some("run" | "exec" | "dlx"))
            && !self.positionals.is_empty()
    }

    fn into_invocation(self, entry_point: EntryPoint) -> Invocation {
        let mut flags = self.tokens.parse();
        let command = if self.ambiguous { None } else { self.command };
        let link_without_args =
            command.as_deref() == Some("link") && self.positionals.is_empty();
        let global = (self.global || link_without_args) && !self.project_location;
        let reads_global =
            matches!(command.as_deref(), Some("config" | "get" | "set")) && !self.project_location;
        finalize_scanned_paths(&mut flags.paths, self.workspace_root, global, reads_global);
        let skip_pm = should_skip_invocation(
            entry_point,
            command.as_deref(),
            &self.positionals,
            self.ambiguous,
            reads_global,
        );
        let warnings = ConfigWarnings::of_invocation(
            entry_point,
            command.as_deref(),
            &self.positionals,
            global,
            skip_pm,
        );
        build_invocation_from_flags(
            flags,
            entry_point,
            command,
            global,
            self.project_location,
            skip_pm,
            warnings,
            self.ambiguous,
        )
    }

    fn absorb(
        &mut self,
        token: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
        next_is_command: bool,
        entry_point: EntryPoint,
    ) -> Option<usize> {
        if self.absorb_scope_flag(token, next, options) && self.after_undeclared_option {
            return None;
        }
        self.after_undeclared_option = may_take_undeclared_value(token, options);
        self.absorb_token(token, next, options, next_is_command, entry_point)
    }

    /// Read `--global`, `-g` in a short cluster, `--workspace-root` / `-w`, or
    /// `--location`, returning whether the token was a global/location scope flag.
    fn absorb_scope_flag(&mut self, token: &str, next: Option<&OsStr>, options: &ArgTable) -> bool {
        if let Some((value, _)) = long_value(token, "location", next) {
            self.project_location = value == "project";
            if self.project_location {
                self.global = false;
            }
            return true;
        }
        let shorts = token
            .strip_prefix('-')
            .filter(|shorts| !shorts.starts_with('-'));
        let global = token == "--global"
            || shorts.is_some_and(|cluster| {
                cluster_options(cluster, options).any(|short| short == 'g')
            });
        let workspace_root = token == "--workspace-root"
            || shorts.is_some_and(|cluster| {
                cluster_options(cluster, options).any(|short| short == 'w')
            });
        self.global |= global;
        self.workspace_root |= workspace_root;
        global
    }

    /// Read one token, returning how many argv tokens it consumed, or
    /// `None` when it leaves the command name ambiguous.
    fn absorb_token(
        &mut self,
        token: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
        next_is_command: bool,
        entry_point: EntryPoint,
    ) -> Option<usize> {
        if !token.starts_with('-') {
            if self.command.is_none() {
                self.command = Some(canonical_command_name(token));
            } else {
                self.positionals.push(token.to_string());
            }
            return Some(1);
        }
        let width = self.tokens
            .offer(token, next, options)
            .unwrap_or_else(|| if consumes_next_token(token, options) { 2 } else { 1 });
        if self.command.is_some() || entry_point == EntryPoint::Version {
            return Some(width);
        }
        // An undeclared option right before a command name is read as a
        // flag without a value, as pnpm v11 reads it.
        declared_option_width(token, options).or_else(|| next_is_command.then_some(1))
    }
}

fn finalize_scanned_paths(
    paths: &mut CliPathArgs,
    workspace_root: bool,
    global: bool,
    reads_global: bool,
) {
    if workspace_root
        && !global
        && !paths.dir_from_command_line
        && let Ok(root) = find_workspace_root_dir(&paths.dir)
    {
        paths.dir = root;
        paths.dir_from_command_line = true;
    }
    if reads_global {
        paths.ignore_workspace = true;
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "constructs Invocation from scanned bootstrap flags and entry-point state"
)]
fn build_invocation_from_flags(
    flags: bootstrap_argv::BootstrapArgv,
    entry_point: EntryPoint,
    command: Option<String>,
    global: bool,
    project_location: bool,
    skip_pm_handling: bool,
    warnings: ConfigWarnings,
    ambiguous: bool,
) -> Invocation {
    let frozen_lockfile = match command.as_deref() {
        Some("ci") => Some(true),
        _ => flags.lockfile.frozen_flag(),
    };
    let color = flags.presentation.color.or_else(|| {
        flags.presentation.no_color.then_some(ColorMode::Never)
    });
    let reporter = flags.reporter_flags();
    Invocation {
        entry_point,
        paths: flags.paths,
        command,
        frozen_lockfile,
        lockfile_dir: flags.lockfile.directory.lockfile_dir,
        offline: typed_flag(flags.network_cache.offline, flags.network_cache.no_offline),
        prefer_offline: typed_flag(
            flags.network_cache.prefer_offline,
            flags.network_cache.no_prefer_offline,
        ),
        color,
        no_color: flags.presentation.no_color,
        network: flags.network,
        global,
        project_location,
        skip_pm_handling,
        check_runtimes: entry_point == EntryPoint::Command,
        reporter,
        warnings,
        ambiguous,
    }
}

fn should_skip_invocation(
    entry_point: EntryPoint,
    command: Option<&str>,
    positionals: &[String],
    ambiguous: bool,
    reads_global_config: bool,
) -> bool {
    let is_pm_only_add = command == Some("add")
        && !positionals.is_empty()
        && positionals
            .iter()
            .all(|spec| crate::engine_pm::pin::declared_package_manager(spec).is_some());
    let is_engine_update_or_outdated =
        matches!(command, Some("update" | "up" | "upgrade" | "outdated"))
            && crate::cli_args::engine_deps::has_only_engine_selectors(positionals);
    ambiguous
        || (entry_point != EntryPoint::SelfUpdate && command.is_some_and(should_skip_command_name))
        || is_pm_only_add
        || is_engine_update_or_outdated
        || reads_global_config
}

/// pnpm treats `--global` as an opt-out of the project's package manager
/// and runtime pins — a global install does not belong to the project.
pub(crate) fn is_global(command: &CliCommand) -> bool {
    // `pnpm link` with no arguments links the current project into the
    // global directory.
    command.is_global()
        || matches!(command, CliCommand::Link(args) if args.package_paths.is_empty())
}

/// The commands pnpm marks with `skipPackageManagerCheck`, plus `setup` —
/// they either predate the project's pins (`setup`), inspect pnpm itself
/// (`store`, `doctor`, and so on), or run something that is not the project
/// (`dlx`).
pub(crate) fn should_skip_command(command: &CliCommand) -> bool {
    match command {
        CliCommand::Add(args) => {
            !args.package_names.is_empty()
                && args.package_names
                    .iter()
                    .all(|request| {
                        crate::engine_pm::pin::declared_package_manager(request.selector())
                            .is_some()
                    })
        }
        CliCommand::Update(args) => {
            crate::cli_args::engine_deps::has_only_engine_selectors(&args.packages)
        }
        CliCommand::Outdated(args) => {
            crate::cli_args::engine_deps::has_only_engine_selectors(&args.packages)
        }
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
        | CliCommand::With(_) => true,
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
