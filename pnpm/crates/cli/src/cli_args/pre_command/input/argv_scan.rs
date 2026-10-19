use super::{
    ArgTable, OsStr, OsString, ReporterFlags, SwitchInput, bootstrap_argv::BootstrapTokens,
    consumes_next_token, long_value,
};
use crate::{boolean_negations::with_boolean_negations, cli_args::CliArgs};
use clap::CommandFactory;

/// The `--version` path. Only the flags typed before the command name are
/// read, as there is no command to act on.
pub(in crate::cli_args::pre_command) struct VersionArgv {
    pub(in crate::cli_args::pre_command) switch: SwitchInput,
    pub(in crate::cli_args::pre_command) reporter: ReporterFlags,
}

impl VersionArgv {
    pub(in crate::cli_args::pre_command) fn scan(argv: &[OsString]) -> Self {
        let options = ArgTable::top_level(crate::cli_args::grammar());
        let mut tokens = BootstrapTokens::default();
        let command = absorb_until_command(argv, &options, &mut tokens);
        let flags = tokens.parse();
        Self { reporter: flags.reporter_flags(), switch: flags.into_switch_input(command) }
    }
}

/// Collect the bootstrap flags before the command name, and return the
/// name itself.
fn absorb_until_command(
    argv: &[OsString],
    options: &ArgTable,
    tokens: &mut BootstrapTokens,
) -> Option<String> {
    let mut index = 1;
    while index < argv.len() {
        let Some(token) = argv[index].to_str() else {
            // A non-UTF-8 token is not a flag this pass knows, and naming
            // no command keeps the caller out of the skip list.
            return Some(String::new());
        };
        if token == "--" {
            return None;
        }
        if !token.starts_with('-') {
            return Some(canonical_command_name(token));
        }
        let next = argv
            .get(index + 1)
            .map(OsString::as_os_str);
        index += tokens
            .offer(token, next, options)
            .unwrap_or_else(|| if consumes_next_token(token, options) { 2 } else { 1 });
    }
    None
}

/// A command line clap rejected, read without clap. Clap never relocated
/// its flags around a parsed command, so they are read on both sides of
/// the command name.
pub(in crate::cli_args::pre_command) struct UnparsedArgv {
    pub(in crate::cli_args::pre_command) switch: SwitchInput,
    pub(in crate::cli_args::pre_command) reporter: ReporterFlags,
    /// `--global`, or `-g` in a short cluster such as `-gE`.
    pub(in crate::cli_args::pre_command) global: bool,
    /// `--location project`.
    pub(in crate::cli_args::pre_command) project_location: bool,
}

impl UnparsedArgv {
    /// `None` when the command line cannot be read reliably: an option
    /// that no command declares comes before the command name but not
    /// right before it, or one the named command does not declare comes
    /// right before a flag that sets its scope, and whether that option
    /// takes the next token is unknown.
    /// A non-UTF-8 token is `None` too.
    pub(in crate::cli_args::pre_command) fn scan(argv: &[OsString]) -> Option<Self> {
        let cli = with_boolean_negations(CliArgs::command());
        let mut options = every_option(&cli);
        let mut scan = UnparsedScan::default();
        let mut index = 1;
        while index < argv.len() {
            let token = argv[index].to_str()?;
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
            index += scan.absorb(token, next, &options, next_is_command)?;
            if let Some(command) = scan.command.as_deref().filter(|_| !named_command) {
                options = command_options(&cli, command);
            }
        }
        let flags = scan.tokens.parse();
        Some(Self {
            reporter: flags.reporter_flags(),
            switch: flags.into_switch_input(scan.command),
            global: scan.global,
            project_location: scan.project_location,
        })
    }
}

/// The state of [`UnparsedArgv::scan`] as it walks the command line.
#[derive(Default)]
struct UnparsedScan {
    command: Option<String>,
    tokens: BootstrapTokens,
    global: bool,
    project_location: bool,
    after_undeclared_option: bool,
}

impl UnparsedScan {
    fn absorb(
        &mut self,
        token: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
        next_is_command: bool,
    ) -> Option<usize> {
        if self.absorb_scope_flag(token, next, options) && self.after_undeclared_option {
            return None;
        }
        self.after_undeclared_option = may_take_undeclared_value(token, options);
        self.absorb_token(token, next, options, next_is_command)
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

    /// Read one token, returning how many argv tokens it consumed, or
    /// `None` when it leaves the command name ambiguous.
    fn absorb_token(
        &mut self,
        token: &str,
        next: Option<&OsStr>,
        options: &ArgTable,
        next_is_command: bool,
    ) -> Option<usize> {
        if !token.starts_with('-') {
            self.command.get_or_insert_with(|| canonical_command_name(token));
            return Some(1);
        }
        let width = self.tokens
            .offer(token, next, options)
            .unwrap_or_else(|| if consumes_next_token(token, options) { 2 } else { 1 });
        if self.command.is_some() {
            return Some(width);
        }
        // An undeclared option right before a command name is read as a
        // flag without a value, as pnpm v11 reads it.
        declared_option_width(token, options).or_else(|| next_is_command.then_some(1))
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
