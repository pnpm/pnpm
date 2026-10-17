use super::{
    ArgTable, CliNetworkArgs, CliPathArgs, OsStr, OsString, PathBuf, PinFlags, PresentationArgs,
    ReporterFlags, SwitchFlags, SwitchInput,
};
use crate::{
    boolean_negations::with_boolean_negations,
    cli_args::{install::InstallLockfileArgs, install_options::OfflineArgs},
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
    paths: CliPathArgs,
    #[clap(flatten)]
    network: CliNetworkArgs,
    #[clap(flatten)]
    presentation: PresentationArgs,
    #[clap(flatten)]
    lockfile: InstallLockfileArgs,
    #[clap(flatten)]
    network_cache: OfflineArgs,
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

    /// The switch input these flags describe for `command`. `pnpm ci` is a
    /// frozen install whether or not the flag was typed.
    pub(in crate::cli_args::pre_command) fn into_switch_input(
        self,
        command: Option<String>,
    ) -> SwitchInput {
        let frozen_lockfile = match command.as_deref() {
            Some("ci") => Some(true),
            _ => self.lockfile.frozen_flag(),
        };
        SwitchInput::from_flags(SwitchFlags {
            command,
            pin_flags: PinFlags::of_install_flags(&self.lockfile, &self.network_cache),
            paths: self.paths,
            network: self.network,
            presentation: &self.presentation,
            frozen_lockfile,
        })
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
