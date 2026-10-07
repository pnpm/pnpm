use super::{
    ArgTable, OsStr, OsString, PathBuf, PinFlags, SwitchInput, SwitchPaths, boolean_flag,
    long_value,
};
use crate::{boolean_negations::with_boolean_negations, cli_args::CliArgs};
use clap::CommandFactory;

/// A command line clap rejected, read without clap. Clap never relocated
/// its flags around a parsed command, so they are read on both sides of
/// the command name.
pub(in crate::cli_args::pre_command) struct UnparsedArgv {
    pub(in crate::cli_args::pre_command) switch: SwitchInput,
    /// `--global`, or `-g` in a short cluster such as `-gE`.
    pub(in crate::cli_args::pre_command) global: bool,
    /// `--location project`.
    pub(in crate::cli_args::pre_command) project_location: bool,
    after_undeclared_option: bool,
}

impl UnparsedArgv {
    /// `None` when the command line cannot be read reliably: an option
    /// that no command declares comes before the command name, or one the
    /// named command does not declare comes right before a flag that sets
    /// its scope, and whether that option takes the next token is unknown.
    /// A non-UTF-8 token is `None` too.
    pub(in crate::cli_args::pre_command) fn scan(argv: &[OsString]) -> Option<Self> {
        let cli = with_boolean_negations(CliArgs::command());
        let mut options = every_option(&cli);
        let mut scan = Self {
            switch: SwitchInput::unscanned(),
            global: false,
            project_location: false,
            after_undeclared_option: false,
        };
        let mut index = 1;
        while index < argv.len() {
            let token = argv[index].to_str()?;
            if token == "--" {
                break;
            }
            let next = argv
                .get(index + 1)
                .map(OsString::as_os_str);
            let named_command = scan.switch.command.is_some();
            index += scan.absorb(token, next, &options)?;
            if let Some(command) = scan.switch.command.as_deref().filter(|_| !named_command) {
                options = command_options(&cli, command);
            }
        }
        if scan.switch.command.as_deref() == Some("ci") {
            scan.switch.frozen_lockfile = Some(true);
        }
        Some(scan)
    }

    fn absorb(&mut self, token: &str, next: Option<&OsStr>, options: &ArgTable) -> Option<usize> {
        if self.absorb_scope_flag(token, next, options) && self.after_undeclared_option {
            return None;
        }
        self.after_undeclared_option = may_take_undeclared_value(token, options);
        self.switch.absorb_unparsed_token(token, next, options)
    }

    /// Read `--global`, `-g` in a short cluster, or `--location`,
    /// returning whether the token was one of them.
    fn absorb_scope_flag(&mut self, token: &str, next: Option<&OsStr>, options: &ArgTable) -> bool {
        if let Some((value, _)) = long_value(token, "location", next) {
            self.project_location = value == "project";
            return true;
        }
        let global = token == "--global"
            || token
                .strip_prefix('-')
                .filter(|shorts| !shorts.starts_with('-'))
                .is_some_and(|shorts| cluster_options(shorts, options).any(|short| short == 'g'));
        self.global |= global;
        global
    }
}

impl SwitchInput {
    /// The `--version` path. Only the flags typed before the command name
    /// are read, as there is no command to act on.
    pub(in crate::cli_args::pre_command) fn from_version_argv(argv: &[OsString]) -> Self {
        let global_options = ArgTable::top_level(crate::cli_args::grammar());
        let mut input = Self::unscanned();
        input.absorb_until_command(argv, &global_options);
        input
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
            self.command.get_or_insert_with(|| canonical_command_name(token));
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
fn every_option(cli: &clap::Command) -> ArgTable {
    let mut options = ArgTable::top_level(cli);
    options.absorb_subcommands(cli);
    options
}

/// The options `command` accepts. An option only another command
/// declares is undeclared here, so its width stays unknown.
fn command_options(cli: &clap::Command, command: &str) -> ArgTable {
    match cli.find_subcommand(command) {
        Some(subcommand) => ArgTable::for_subcommand(cli, subcommand),
        None => ArgTable::top_level(cli),
    }
}

/// Whether `token` is an option no command declares that might take the
/// next token as its value. `--option=value` carries its value already.
fn may_take_undeclared_value(token: &str, options: &ArgTable) -> bool {
    let attached_long_value = token.starts_with("--") && token.contains('=');
    token.starts_with('-')
        && !attached_long_value
        && declared_option_width(token, options).is_none()
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

/// The command's own name for `name`, which may be one of its aliases.
fn canonical_command_name(name: &str) -> String {
    crate::cli_args::grammar()
        .find_subcommand(name)
        .map_or(name, clap::Command::get_name)
        .to_string()
}

/// The letters of a short cluster that name options: up to and including
/// the first one that takes a value, whose value is the rest of the token.
fn cluster_options<'a>(shorts: &'a str, options: &'a ArgTable) -> impl Iterator<Item = char> + 'a {
    let mut value_follows = false;
    shorts
        .chars()
        .take_while(move |&short| {
            let named = !value_follows;
            value_follows = options.short_consumes_value(short) != Some(false);
            named
        })
}
