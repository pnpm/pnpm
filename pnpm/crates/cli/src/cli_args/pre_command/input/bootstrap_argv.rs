use super::{
    ArgTable, CliNetworkArgs, CliPathArgs, OsStr, OsString, PathBuf, PresentationArgs,
    ReporterFlags,
};
use crate::{
    boolean_negations::with_boolean_negations,
    cli_args::{grammar, install::InstallLockfileArgs, install_options::OfflineArgs},
};
use clap::{CommandFactory, FromArgMatches, parser::ValueSource};
use std::sync::OnceLock;

/// The flags the pre-command pass reads from a command line it scans
/// itself, declared by the clap groups `CliArgs` and the install family
/// flatten. A flag added to one of these groups is
/// read on every path into the pass without a second edit;
/// `every_global_flag_is_read_before_the_switch_or_after_it` makes a new
/// group choose a side.
#[derive(Debug, clap::Parser)]
#[clap(no_binary_name = true, args_override_self = true)]
pub(in crate::cli_args::pre_command) struct BootstrapArgv {
    #[clap(flatten)]
    pub(super) paths: CliPathArgs,
    #[clap(flatten)]
    pub(super) network: CliNetworkArgs,
    #[clap(flatten)]
    pub(super) presentation: PresentationArgs,
    #[clap(flatten)]
    pub(super) lockfile: InstallLockfileArgs,
    #[clap(flatten)]
    pub(super) network_cache: OfflineArgs,
}

impl BootstrapArgv {
    pub(in crate::cli_args::pre_command) fn command() -> &'static clap::Command {
        static COMMAND: OnceLock<clap::Command> = OnceLock::new();
        COMMAND.get_or_init(|| with_boolean_negations(<Self as CommandFactory>::command()))
    }

    fn options() -> &'static ArgTable {
        static OPTIONS: OnceLock<ArgTable> = OnceLock::new();
        OPTIONS.get_or_init(|| ArgTable::subcommand(Self::command()))
    }

    pub(in crate::cli_args::pre_command) fn reporter_flags(&self) -> ReporterFlags {
        ReporterFlags { reporter: self.presentation.reporter, loglevel: self.presentation.loglevel }
    }
}

/// The argv tokens that name a [`BootstrapArgv`] flag, one entry per flag
/// with its value, collected by a scan that walks the rest of the command
/// line itself.
#[derive(Default)]
pub(in crate::cli_args::pre_command) struct BootstrapTokens(Vec<Vec<OsString>>);

impl BootstrapTokens {
    /// Keep `token` when it names a bootstrap flag, returning how many argv
    /// tokens the flag and its value span. `None` leaves the width to the
    /// caller: the token names no bootstrap flag, or a short cluster names
    /// one that does not take the next token. `options` is the grammar the
    /// caller reads the rest of the line with, which says where a cluster's
    /// value starts.
    pub(in crate::cli_args::pre_command) fn offer(
        &mut self,
        token: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
    ) -> Option<usize> {
        match token.strip_prefix("--") {
            Some(long) => self.offer_long(token, long, next),
            None => self.offer_short_cluster(token.strip_prefix('-')?, next, options),
        }
    }

    fn offer_long(&mut self, token: &str, long: &str, next: Option<&OsStr>) -> Option<usize> {
        let (name, value_attached) = long
            .split_once('=')
            .map_or((long, false), |(name, _)| (name, true));
        let consumes_value = BootstrapArgv::options().long_consumes_value(name)?;
        if !consumes_value || value_attached {
            self.0.push(vec![OsString::from(token)]);
            return Some(1);
        }
        self.0.push(
            std::iter::once(OsString::from(token))
                .chain(next.map(OsStr::to_os_string))
                .collect(),
        );
        Some(2)
    }

    /// Keep the bootstrap letters of a short cluster such as `-rC <dir>`.
    /// The cluster ends at its first value-taking letter, whose value is
    /// the rest of the token or, when nothing follows it, the next token.
    fn offer_short_cluster(
        &mut self,
        shorts: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
    ) -> Option<usize> {
        for (index, short) in shorts.char_indices() {
            let rest = &shorts[index + short.len_utf8()..];
            let Some(consumes_value) = BootstrapArgv::options().short_consumes_value(short) else {
                if options.short_consumes_value(short) != Some(false) {
                    return None;
                }
                continue;
            };
            let flag = OsString::from(format!("-{short}"));
            if !consumes_value {
                self.0.push(vec![flag]);
                continue;
            }
            if !rest.is_empty() {
                self.0.push(vec![OsString::from(format!("-{short}{rest}"))]);
                return None;
            }
            self.0.push(
                std::iter::once(flag)
                    .chain(next.map(OsStr::to_os_string))
                    .collect(),
            );
            return Some(2);
        }
        None
    }

    /// Parse the kept flags. A flag whose value clap rejects is dropped, so
    /// it costs none of the others: the command line is one this pnpm could
    /// not parse to begin with. A `--dir` that none of them gave resolves to
    /// the local prefix, as it does for a parsed command line.
    pub(in crate::cli_args::pre_command) fn parse(self) -> BootstrapArgv {
        let command = BootstrapArgv::command();
        let matches = command
            .clone()
            .try_get_matches_from(self.0.iter().flatten())
            .unwrap_or_else(|_| {
                let accepted = self.0
                    .iter()
                    .filter(|flag| {
                        command
                            .clone()
                            .try_get_matches_from(*flag)
                            .is_ok()
                    })
                    .flatten();
                command
                    .clone()
                    .ignore_errors(true)
                    .try_get_matches_from(accepted)
                    .expect("`ignore_errors` keeps every bootstrap parse from failing")
            });
        let mut flags = BootstrapArgv::from_arg_matches(&matches)
            .expect("the matches come from the command `BootstrapArgv` derives");
        flags.paths.dir_from_command_line =
            matches.value_source("dir") == Some(ValueSource::CommandLine);
        if !flags.paths.dir_from_command_line {
            flags.paths.dir = local_prefix_or_cwd();
        }
        flags
    }
}

/// The `--dir` a command line without one resolves to.
///
/// Degrades to `.`, which the caller canonicalizes: a cwd that cannot be
/// resolved fails there, with the path it was given.
fn local_prefix_or_cwd() -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::cli_args::prefix::find_local_prefix(&cwd).ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Every option of every command, with the `--no-` spellings, so a
/// token's width is known wherever on the command line it was typed.
pub(super) fn every_option(cli: &clap::Command) -> ArgTable {
    let mut options = ArgTable::top_level(cli);
    options.absorb_subcommands(cli);
    options
}

/// The options `command` accepts. An option only another command
/// declares is undeclared here, so its width stays unknown.
pub(super) fn command_options(cli: &clap::Command, command: &str) -> ArgTable {
    match cli.find_subcommand(command) {
        Some(subcommand) => ArgTable::for_subcommand(cli, subcommand),
        None => ArgTable::top_level(cli),
    }
}

/// Whether `token` is an option no command declares that might take the
/// next token as its value. `--option=value` carries its value already.
pub(super) fn may_take_undeclared_value(token: &str, options: &ArgTable) -> bool {
    let attached_long_value = token.starts_with("--") && token.contains('=');
    token.starts_with('-')
        && !attached_long_value
        && declared_option_width(token, options).is_none()
}

/// How many argv tokens a declared option spans, or `None` for an option
/// no command declares.
pub(super) fn declared_option_width(token: &str, options: &ArgTable) -> Option<usize> {
    match token.strip_prefix("--") {
        Some(name) => declared_long_width(name, options),
        None => declared_short_cluster_width(token.strip_prefix('-')?, options),
    }
}

fn declared_long_width(name: &str, options: &ArgTable) -> Option<usize> {
    let (name, value_attached) = name
        .split_once('=')
        .map_or((name, false), |(name, _)| (name, true));
    let consumes_value = options.long_consumes_value(name)?;
    Some(if consumes_value && !value_attached { 2 } else { 1 })
}

/// A cluster ends at its first value-taking short option, whose value is
/// the rest of the token or, when nothing follows it, the next token.
/// `None` when a letter before it is undeclared.
fn declared_short_cluster_width(shorts: &str, options: &ArgTable) -> Option<usize> {
    for (index, short) in shorts.char_indices() {
        if options.short_consumes_value(short)? {
            let value_attached = index + short.len_utf8() < shorts.len();
            return Some(if value_attached { 1 } else { 2 });
        }
    }
    Some(1)
}

/// The command's own name for `name`, which may be one of its aliases.
pub(super) fn canonical_command_name(name: &str) -> String {
    grammar()
        .find_subcommand(name)
        .map_or(name, clap::Command::get_name)
        .to_string()
}

/// The letters of a short cluster that name options: up to and including
/// the first one that takes a value, whose value is the rest of the token.
pub(super) fn cluster_options<'a>(
    shorts: &'a str,
    options: &'a ArgTable,
) -> impl Iterator<Item = char> + 'a {
    let mut value_follows = false;
    shorts
        .chars()
        .take_while(move |&short| {
            let named = !value_follows;
            value_follows = options.short_consumes_value(short) != Some(false);
            named
        })
}

/// A `--flag` / `--no-flag` pair as typed on the command line, or `None`
/// when neither spelling was.
pub(super) fn typed_flag(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

pub(super) fn long_value<'a>(
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

pub(super) fn consumes_next_token(token: &str, global_options: &ArgTable) -> bool {
    declared_option_width(token, global_options) == Some(2)
}
