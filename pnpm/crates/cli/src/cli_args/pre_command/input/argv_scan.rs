use super::{ArgTable, OsString, PinFlags, SwitchInput, SwitchPaths};

impl SwitchInput {
    /// The `--version` path. Only the flags typed before the command name
    /// are read, as there is no command to act on.
    pub(in crate::cli_args::pre_command) fn from_version_argv(argv: &[OsString]) -> Self {
        Self::scan_argv(argv, false)
    }

    /// A command line clap rejected. Its flags were not relocated around a
    /// parsed command, so they are read on both sides of the command name.
    pub(in crate::cli_args::pre_command) fn from_unparsed_argv(argv: &[OsString]) -> Self {
        Self::scan_argv(argv, true)
    }

    fn scan_argv(argv: &[OsString], past_command: bool) -> Self {
        let global_options = ArgTable::top_level(crate::cli_args::grammar());
        let mut input = Self {
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
        };
        let resume_at = input.absorb_until_command(argv, &global_options);
        if past_command && let Some(rest) = argv.get(resume_at..) {
            input.absorb_after_command(rest, &global_options);
        }
        input
    }

    /// Read the flags before the command name, returning the argv index to
    /// resume at past it, or the argv length when nothing follows to read.
    fn absorb_until_command(&mut self, argv: &[OsString], global_options: &ArgTable) -> usize {
        let mut index = 1;
        while index < argv.len() {
            let Some(token) = argv[index].to_str() else {
                // A non-UTF-8 token is not a flag this pass knows, and
                // naming no command keeps the caller out of the skip list.
                self.command = Some(String::new());
                return argv.len();
            };
            if token == "--" {
                return argv.len();
            }
            if !token.starts_with('-') {
                self.command = Some(token.to_string());
                return index + 1;
            }
            let next = argv
                .get(index + 1)
                .map(OsString::as_os_str);
            index += self.absorb_global_flag(token, next, global_options);
        }
        index
    }

    /// Read the flags after the command name, stepping over its positionals.
    fn absorb_after_command(&mut self, rest: &[OsString], global_options: &ArgTable) {
        let mut index = 0;
        while let Some(token) = rest
            .get(index)
            .and_then(|token| token.to_str())
            .filter(|token| *token != "--")
        {
            let next = rest
                .get(index + 1)
                .map(OsString::as_os_str);
            index += if token.starts_with('-') {
                self.absorb_global_flag(token, next, global_options)
            } else {
                1
            };
        }
    }
}

/// Whether `-g` / `--global` was typed before any `--` separator.
pub(in crate::cli_args::pre_command) fn argv_requests_global(argv: &[OsString]) -> bool {
    argv.iter()
        .skip(1)
        .map_while(|token| token.to_str())
        .take_while(|token| *token != "--")
        .any(|token| token == "-g" || token == "--global")
}
