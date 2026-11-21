// Copyright 2018-2025 the Deno authors. MIT license.

use futures::future::LocalBoxFuture;

use crate::shell::types::{
    EnvChange,
    ExecuteResult,
    ShellOptions,
};

use super::{
    ShellCommand,
    ShellCommandContext,
};

pub struct ShoptCommand;

impl ShellCommand for ShoptCommand {
    fn execute(&self, mut context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        Box::pin(async move {
            let mut set_mode = None; // None = query, Some(true) = -s, Some(false) = -u
            let mut options_to_change = Vec::new();

            for arg in context.args.into_iter().peekable() {
                let arg_str = arg.to_string_lossy();
                match arg_str.as_ref() {
                    "-s" => {
                        if set_mode == Some(false) {
                            let _ = context.stderr.write_line(
                                "shopt: cannot set and unset options simultaneously",
                            );
                            return ExecuteResult::from_exit_code(1);
                        }
                        set_mode = Some(true);
                    }
                    "-u" => {
                        if set_mode == Some(true) {
                            let _ = context.stderr.write_line(
                                "shopt: cannot set and unset options simultaneously",
                            );
                            return ExecuteResult::from_exit_code(1);
                        }
                        set_mode = Some(false);
                    }
                    _ => {
                        // treat as option name
                        match parse_option_name(&arg_str) {
                            Some(opt) => options_to_change.push(opt),
                            None => {
                                let _ = context.stderr.write_line(&format!(
                                    "shopt: {}: invalid shell option name",
                                    arg_str
                                ));
                                return ExecuteResult::from_exit_code(1);
                            }
                        }
                    }
                }
            }

            match set_mode {
                Some(enabled) => {
                    // set or unset mode
                    if options_to_change.is_empty() {
                        let _ = context.stderr.write_line("shopt: option name required");
                        return ExecuteResult::from_exit_code(1);
                    }

                    let changes: Vec<EnvChange> = options_to_change
                        .into_iter()
                        .map(|opt| EnvChange::SetOption(opt, enabled))
                        .collect();

                    ExecuteResult::Continue(0, changes, Vec::new())
                }
                None => {
                    // query mode - print option status
                    let current_options = context.state.shell_options();

                    if options_to_change.is_empty() {
                        // print all options (alphabetical order)
                        let _ = context.stdout.write_line(&format!(
                            "failglob\t{}",
                            if current_options.contains(ShellOptions::FAILGLOB) {
                                "on"
                            } else {
                                "off"
                            }
                        ));
                        let _ = context.stdout.write_line(&format!(
                            "globstar\t{}",
                            if current_options.contains(ShellOptions::GLOBSTAR) {
                                "on"
                            } else {
                                "off"
                            }
                        ));
                        let _ = context.stdout.write_line(&format!(
                            "nullglob\t{}",
                            if current_options.contains(ShellOptions::NULLGLOB) {
                                "on"
                            } else {
                                "off"
                            }
                        ));
                        ExecuteResult::from_exit_code(0)
                    } else {
                        // print specified options and return non-zero if any are off
                        let mut any_off = false;
                        for opt in options_to_change {
                            let is_on = current_options.contains(opt);
                            if !is_on {
                                any_off = true;
                            }
                            let name = option_to_name(opt);
                            let _ = context.stdout.write_line(&format!(
                                "{}\t{}",
                                name,
                                if is_on { "on" } else { "off" }
                            ));
                        }
                        ExecuteResult::from_exit_code(if any_off { 1 } else { 0 })
                    }
                }
            }
        })
    }
}

fn parse_option_name(name: &str) -> Option<ShellOptions> {
    match name {
        "nullglob" => Some(ShellOptions::NULLGLOB),
        "failglob" => Some(ShellOptions::FAILGLOB),
        "globstar" => Some(ShellOptions::GLOBSTAR),
        _ => None,
    }
}

fn option_to_name(opt: ShellOptions) -> &'static str {
    if opt == ShellOptions::NULLGLOB {
        "nullglob"
    } else if opt == ShellOptions::FAILGLOB {
        "failglob"
    } else if opt == ShellOptions::GLOBSTAR {
        "globstar"
    } else {
        "unknown"
    }
}
