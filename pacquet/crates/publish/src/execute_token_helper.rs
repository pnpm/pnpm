//! Port of `executeTokenHelper.ts`: run a configured `tokenHelper` command and
//! return the auth token it prints.

use std::io;

use pacquet_reporter::Reporter;

use crate::{capabilities::RunCommand, global_log::global_warn};

/// Run the `tokenHelper` command (`[cmd, ...args]`) and return its stdout as a
/// bare token. Each non-empty stderr line is surfaced as a warning, and a
/// leading `Bearer ` scheme is stripped (libnpmpublish adds the scheme
/// itself). Ports TS `executeTokenHelper`.
pub fn execute_token_helper<Sys, R>(token_helper: &[String]) -> io::Result<String>
where
    Sys: RunCommand,
    R: Reporter,
{
    let Some((program, args)) = token_helper.split_first() else {
        return Ok(String::new());
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = Sys::run(program, &args, None)?;

    let stderr = output.stderr.trim_end();
    if !stderr.trim().is_empty() {
        for line in stderr.split('\n') {
            global_warn::<R>(&format!("(tokenHelper stderr) {line}"));
        }
    }

    Ok(strip_bearer_prefix(output.stdout.trim()).to_owned())
}

/// Strip a leading `Bearer ` (case-insensitive, requiring at least one
/// trailing whitespace) from `token`. Mirrors the TS `replace(/^Bearer\s+/i, '')`.
fn strip_bearer_prefix(token: &str) -> &str {
    let Some(after_scheme) = token.get(..6).filter(|head| head.eq_ignore_ascii_case("bearer"))
    else {
        return token;
    };
    debug_assert_eq!(after_scheme.len(), 6);
    let rest = &token[6..];
    let trimmed = rest.trim_start_matches(char::is_whitespace);
    // The regex requires `\s+`, so only strip when whitespace actually followed.
    if trimmed.len() < rest.len() { trimmed } else { token }
}

#[cfg(test)]
mod tests {
    use super::{execute_token_helper, strip_bearer_prefix};
    use crate::capabilities::{CommandOutput, RunCommand};
    use pacquet_reporter::SilentReporter;
    use pretty_assertions::assert_eq;
    use std::{io, path::Path};

    #[test]
    fn strips_bearer_prefix_case_insensitively() {
        assert_eq!(strip_bearer_prefix("Bearer abc"), "abc");
        assert_eq!(strip_bearer_prefix("bearer   xyz"), "xyz");
    }

    #[test]
    fn keeps_token_without_whitespace_after_scheme() {
        assert_eq!(strip_bearer_prefix("Bearertoken"), "Bearertoken");
        assert_eq!(strip_bearer_prefix("plain"), "plain");
    }

    #[test]
    fn returns_trimmed_stdout() {
        struct Helper;
        impl RunCommand for Helper {
            fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
                Ok(CommandOutput {
                    success: true,
                    stdout: "  Bearer secret-token\n".to_owned(),
                    stderr: String::new(),
                })
            }
        }
        let token = execute_token_helper::<Helper, SilentReporter>(&["helper".to_owned()]).unwrap();
        assert_eq!(token, "secret-token");
    }

    #[test]
    fn empty_command_yields_empty_token() {
        struct Unreachable;
        impl RunCommand for Unreachable {
            fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
                unreachable!("an empty tokenHelper must not spawn a process")
            }
        }
        let token = execute_token_helper::<Unreachable, SilentReporter>(&[]).unwrap();
        assert_eq!(token, "");
    }
}
