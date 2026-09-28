use super::{
    ArgTable, OsStr, OsString, PathBuf, PinFlags, SwitchInput, SwitchPaths, boolean_flag,
    long_value,
};
use crate::{boolean_negations::with_boolean_negations, cli_args::CliArgs};
use clap::CommandFactory;

impl SwitchInput {
    /// The `--version` path. Only the flags typed before the command name
    /// are read, as there is no command to act on.
    pub(in crate::cli_args::pre_command) fn from_version_argv(argv: &[OsString]) -> Self {
        let global_options = ArgTable::top_level(crate::cli_args::grammar());
        let mut input = Self::unscanned();
        input.absorb_until_command(argv, &global_options);
        input
    }

    /// A command line clap rejected. Clap never relocated its flags around
    /// a parsed command, so they are read on both sides of the command name.
    ///
    /// `None` when the command name cannot be told apart from an option
    /// value: an option no command declares comes before it, and whether
    /// that option takes the next token is unknown. A non-UTF-8 token is
    /// `None` too.
    pub(in crate::cli_args::pre_command) fn from_unparsed_argv(argv: &[OsString]) -> Option<Self> {
        let options = every_option();
        let mut input = Self::unscanned();
        let mut index = 1;
        while index < argv.len() {
            let token = argv[index].to_str()?;
            if token == "--" {
                break;
            }
            let next = argv
                .get(index + 1)
                .map(OsString::as_os_str);
            index += input.absorb_unparsed_token(token, next, &options)?;
        }
        if input.command.as_deref() == Some("ci") {
            input.frozen_lockfile.get_or_insert(true);
        }
        Some(input)
    }

    fn unscanned() -> Self {
        Self {
            paths: SwitchPaths {
                dir: Self::local_prefix_or_cwd(),
                state_dir: None,
                store_dir: None,
                npmrc_auth_file: None,
            },
            command: None,
            frozen_lockfile: None,
            pin_flags: PinFlags::default(),
            color: None,
            ignore_workspace: false,
        }
    }

    /// Read the flags before the command name, and the name itself.
    fn absorb_until_command(&mut self, argv: &[OsString], global_options: &ArgTable) {
        let mut index = 1;
        while index < argv.len() {
            let Some(token) = argv[index].to_str() else {
                // A non-UTF-8 token is not a flag this pass knows, and
                // naming no command keeps the caller out of the skip list.
                self.command = Some(String::new());
                return;
            };
            if token == "--" {
                return;
            }
            if !token.starts_with('-') {
                self.command = Some(token.to_string());
                return;
            }
            let next = argv
                .get(index + 1)
                .map(OsString::as_os_str);
            index += self.absorb_global_flag(token, next, global_options);
        }
    }

    /// Read one token of a command line clap rejected, returning how many
    /// argv tokens it consumed, or `None` when it leaves the command name
    /// ambiguous.
    fn absorb_unparsed_token(
        &mut self,
        token: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
    ) -> Option<usize> {
        if !token.starts_with('-') {
            self.command.get_or_insert_with(|| token.to_string());
            return Some(1);
        }
        if self.command.is_none() && !is_declared_option(token, options) {
            return None;
        }
        self.absorb_pin_flag(token, next)
            .or_else(|| Some(self.absorb_global_flag(token, next, options)))
    }

    /// Read one of the install-family options the pin record reads (see
    /// [`PinFlags`]), returning how many argv tokens it consumed.
    fn absorb_pin_flag(&mut self, token: &str, next: Option<&OsStr>) -> Option<usize> {
        if let Some(set) = boolean_flag(token, "frozen-lockfile") {
            self.frozen_lockfile = Some(set);
            return Some(1);
        }
        if let Some(set) = boolean_flag(token, "offline") {
            self.pin_flags.offline = Some(set);
            return Some(1);
        }
        if let Some(set) = boolean_flag(token, "prefer-offline") {
            self.pin_flags.prefer_offline = Some(set);
            return Some(1);
        }
        let (value, width) = long_value(token, "lockfile-dir", next)?;
        self.pin_flags.lockfile_dir = Some(PathBuf::from(value));
        Some(width)
    }
}

/// Every option of every command, with the `--no-` spellings, so a
/// token's width is known wherever on the command line it was typed.
fn every_option() -> ArgTable {
    let cli = with_boolean_negations(CliArgs::command());
    let mut options = ArgTable::top_level(&cli);
    options.absorb_subcommands(&cli);
    options
}

fn is_declared_option(token: &str, options: &ArgTable) -> bool {
    if let Some(name) = token.strip_prefix("--") {
        let name = name.split_once('=').map_or(name, |(name, _)| name);
        return options.long_consumes_value(name).is_some();
    }
    token
        .strip_prefix('-')
        .is_some_and(|shorts| {
            shorts
                .chars()
                .all(|short| options.short_consumes_value(short).is_some())
        })
}

/// Whether `-g` / `--global` was typed before any `--` separator.
pub(in crate::cli_args::pre_command) fn argv_requests_global(argv: &[OsString]) -> bool {
    typed_tokens(argv).any(|token| token == "-g" || token == "--global")
}

/// Whether `--location project` was typed before any `--` separator.
pub(in crate::cli_args::pre_command) fn argv_requests_project_location(argv: &[OsString]) -> bool {
    let tokens = typed_tokens(argv).collect::<Vec<_>>();
    tokens
        .iter()
        .enumerate()
        .any(|(index, token)| {
            let next = tokens.get(index + 1).map(OsStr::new);
            long_value(token, "location", next).is_some_and(|(value, _)| value == "project")
        })
}

fn typed_tokens(argv: &[OsString]) -> impl Iterator<Item = &str> {
    argv.iter()
        .skip(1)
        .map_while(|token| token.to_str())
        .take_while(|token| *token != "--")
}
