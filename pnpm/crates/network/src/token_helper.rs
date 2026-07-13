//! Lazy execution of a configured `tokenHelper` command into an
//! `Authorization` header value.
//!
//! `tokenHelper` names an executable pnpm runs to obtain a registry
//! token. [`AuthHeaders`](crate::AuthHeaders) stores the parsed command
//! and runs it — at most once, memoized — only when a lookup actually
//! resolves to that registry, so a command never spawns for a pnpm
//! invocation that makes no matching request. The mapping from the
//! command's stdout to a header mirrors pnpm's `executeTokenHelper`.

use std::{io, process::Command};

/// Captured output of a `tokenHelper` subprocess.
#[derive(Debug, Clone)]
pub struct TokenHelperOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a `tokenHelper` command line (`[program, ...args]`) and captures
/// its output. Injected into [`AuthHeaders`](crate::AuthHeaders) so tests
/// can drive the execution branches without spawning a real process;
/// production uses `run_token_helper_command`.
pub type TokenHelperRunner = fn(&[String]) -> io::Result<TokenHelperOutput>;

/// Failure while turning a `tokenHelper` into a header. The `Display`
/// text carries pnpm's error code so a `tokenHelper` misconfiguration is
/// as recognizable in pacquet's logs as in pnpm's thrown error.
#[derive(Debug)]
pub enum TokenHelperError {
    /// The command could not be spawned at all.
    Spawn { program: String, source: io::Error },
    /// The command exited non-zero (`ERR_PNPM_TOKEN_HELPER_ERROR_STATUS`).
    ErrorStatus { program: String },
    /// The command exited zero but printed nothing
    /// (`ERR_PNPM_TOKEN_HELPER_EMPTY_TOKEN`).
    EmptyToken { program: String },
}

impl std::fmt::Display for TokenHelperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenHelperError::Spawn { program, source } => {
                write!(f, "Failed to run {program:?} as a token helper: {source}")
            }
            TokenHelperError::ErrorStatus { program } => write!(
                f,
                "ERR_PNPM_TOKEN_HELPER_ERROR_STATUS: Error running {program:?} as a token helper",
            ),
            TokenHelperError::EmptyToken { program } => write!(
                f,
                "ERR_PNPM_TOKEN_HELPER_EMPTY_TOKEN: Token helper {program:?} returned an empty token",
            ),
        }
    }
}

/// Execute `command` via `run` and map its stdout to an `Authorization`
/// header value, mirroring pnpm's `executeTokenHelper`:
/// - a non-zero exit is [`TokenHelperError::ErrorStatus`];
/// - an empty stdout (after trailing-whitespace trim) is
///   [`TokenHelperError::EmptyToken`];
/// - a token that already begins with an auth scheme (`^[A-Za-z]+ `) is
///   returned verbatim; otherwise it is prefixed with `Bearer `.
pub fn execute_token_helper(
    command: &[String],
    run: TokenHelperRunner,
) -> Result<String, TokenHelperError> {
    let Some(program) = command.first() else {
        return Err(TokenHelperError::EmptyToken { program: String::new() });
    };
    let output = run(command)
        .map_err(|source| TokenHelperError::Spawn { program: program.clone(), source })?;
    if !output.success {
        return Err(TokenHelperError::ErrorStatus { program: program.clone() });
    }
    let token = output.stdout.trim_end();
    if token.is_empty() {
        return Err(TokenHelperError::EmptyToken { program: program.clone() });
    }
    if starts_with_auth_scheme(token) {
        Ok(token.to_owned())
    } else {
        Ok(format!("Bearer {token}"))
    }
}

/// Production runner: spawn `command` and capture its output. On Windows a
/// `.bat`/`.cmd` program is run through `cmd /C` (those files need a
/// shell), matching pnpm's `shell` handling.
pub fn run_token_helper_command(command: &[String]) -> io::Result<TokenHelperOutput> {
    let Some((program, args)) = command.split_first() else {
        return Ok(TokenHelperOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        });
    };
    let output = build_command(program, args).output()?;
    Ok(TokenHelperOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(windows)]
fn build_command(program: &str, args: &[String]) -> Command {
    let lowercased = program.to_ascii_lowercase();
    if lowercased.ends_with(".bat") || lowercased.ends_with(".cmd") {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(program).args(args);
        return command;
    }
    let mut command = Command::new(program);
    command.args(args);
    command
}

#[cfg(not(windows))]
fn build_command(program: &str, args: &[String]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    command
}

/// Whether `token` begins with an auth scheme: one or more ASCII letters
/// immediately followed by a space, e.g. `Bearer …` or `Basic …`. Mirrors
/// pnpm's `/^[A-Z]+ /i`.
fn starts_with_auth_scheme(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
        index += 1;
    }
    index > 0 && bytes.get(index) == Some(&b' ')
}

#[cfg(test)]
mod tests;
