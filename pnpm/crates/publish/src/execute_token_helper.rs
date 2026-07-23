//! run a configured `tokenHelper` command and
//! return the auth token it prints.
//!
//! The publish PUT now authenticates through the shared
//! [`AuthHeaders`](pacquet_network::AuthHeaders) map, which carries an
//! un-executed `tokenHelper` command per registry and runs it lazily on
//! lookup (see `pacquet_network::token_helper`) — so a `tokenHelper`-only
//! registry is authenticated on publish without this helper. This
//! function is retained for the publish-specific path that needs the bare
//! token (no `Bearer` scheme) rather than a finished header value.

use std::io;

use pacquet_reporter::Reporter;

use crate::{capabilities::RunCommand, global_log::global_warn};

/// Run the `tokenHelper` command (`[cmd, ...args]`) and return its stdout as a
/// bare token. Each non-empty stderr line is surfaced as a warning, and a
/// leading `Bearer ` scheme is stripped (the publish request adds the
/// scheme itself).
pub fn execute_token_helper<Sys, Reporter>(token_helper: &[String]) -> io::Result<String>
where
    Sys: RunCommand,
    Reporter: self::Reporter,
{
    let Some((program, args)) = token_helper.split_first() else {
        return Ok(String::new());
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = Sys::run(program, &args, None)?;

    // A non-zero exit aborts the publish rather than
    // proceeding with an empty token; mirror that with an error.
    if !output.success {
        return Err(io::Error::other(format!(
            "tokenHelper `{program}` exited with a failure: {}",
            output.stderr.trim(),
        )));
    }

    let stderr = output.stderr.trim_end();
    if !stderr.trim().is_empty() {
        for line in stderr.split('\n') {
            global_warn::<Reporter>(&format!("(tokenHelper stderr) {line}"));
        }
    }

    Ok(strip_bearer_prefix(output.stdout.trim()).to_owned())
}

/// Strip a leading `Bearer ` (case-insensitive, requiring at least one
/// trailing whitespace) from `token`.
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
mod tests;
