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
            input.frozen_lockfile = Some(true);
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
        self.absorb_clustered_dir(token, next, options);
        let width = self
            .absorb_pin_flag(token, next)
            .unwrap_or_else(|| self.absorb_global_flag(token, next, options));
        if self.command.is_some() {
            return Some(width);
        }
        declared_option_width(token, options)
    }

    /// Read `-C` inside a short cluster, such as `-rC <dir>`, which
    /// [`SwitchPaths::absorb_flag`] reads only at the start of a token.
    fn absorb_clustered_dir(&mut self, token: &str, next: Option<&OsStr>, options: &ArgTable) {
        let Some(shorts) = token
            .strip_prefix('-')
            .filter(|shorts| !shorts.starts_with('-'))
        else {
            return;
        };
        if let Some(Some(ShortValue { short: 'C', attached })) =
            short_cluster_value(shorts, options)
            && let Some(dir) = attached.map(OsStr::new).or(next)
        {
            self.paths.dir = PathBuf::from(dir);
        }
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

/// How many argv tokens a declared option spans, or `None` for an option
/// no command declares.
fn declared_option_width(token: &str, options: &ArgTable) -> Option<usize> {
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

fn declared_short_cluster_width(shorts: &str, options: &ArgTable) -> Option<usize> {
    Some(match short_cluster_value(shorts, options)? {
        Some(ShortValue { attached: None, .. }) => 2,
        _ => 1,
    })
}

/// The value-taking option that ends a short cluster.
struct ShortValue<'a> {
    short: char,
    /// The rest of the token. `None` when the value is the next token.
    attached: Option<&'a str>,
}

/// A cluster ends at its first value-taking short option. `None` when a
/// letter before it is undeclared, `Some(None)` when no letter takes a value.
fn short_cluster_value<'a>(shorts: &'a str, options: &ArgTable) -> Option<Option<ShortValue<'a>>> {
    for (index, short) in shorts.char_indices() {
        if options.short_consumes_value(short)? {
            let rest = &shorts[index + short.len_utf8()..];
            return Some(Some(ShortValue { short, attached: (!rest.is_empty()).then_some(rest) }));
        }
    }
    Some(None)
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
