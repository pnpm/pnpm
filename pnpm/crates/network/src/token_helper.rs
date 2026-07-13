//! Lazy execution of a configured `tokenHelper` command into an
//! `Authorization` header value.
//!
//! `tokenHelper` names an executable pnpm runs to obtain a registry
//! token. [`AuthHeaders`](crate::AuthHeaders) stores the parsed command
//! and runs it — at most once, memoized — only when a lookup actually
//! resolves to that registry, so a command never spawns for a pnpm
//! invocation that makes no matching request. The mapping from the
//! command's stdout to a header mirrors pnpm's `executeTokenHelper`.

use std::{
    io::{self, Read},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Seconds a `tokenHelper` command may run before it is killed.
const TOKEN_HELPER_TIMEOUT_SECS: u64 = 60;

/// How long a `tokenHelper` command may run before it is killed. A helper
/// only prints a token, so this is a generous bound that turns a hung
/// helper (deadlock, stuck I/O) into a clear error instead of an install
/// that hangs forever. Matches the TypeScript CLI's `tokenHelper` timeout.
pub const TOKEN_HELPER_TIMEOUT: Duration = Duration::from_secs(TOKEN_HELPER_TIMEOUT_SECS);

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
    /// The command did not finish within [`TOKEN_HELPER_TIMEOUT`] and was
    /// killed (`ERR_PNPM_TOKEN_HELPER_TIMEOUT`).
    Timeout { program: String },
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
            TokenHelperError::Timeout { program } => write!(
                f,
                "ERR_PNPM_TOKEN_HELPER_TIMEOUT: Token helper {program:?} timed out after {} ms",
                TOKEN_HELPER_TIMEOUT.as_millis(),
            ),
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
    let output = run(command).map_err(|source| {
        if source.kind() == io::ErrorKind::TimedOut {
            TokenHelperError::Timeout { program: program.clone() }
        } else {
            TokenHelperError::Spawn { program: program.clone(), source }
        }
    })?;
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

/// Production runner: spawn `command`, capture its output, and enforce
/// [`TOKEN_HELPER_TIMEOUT`]. On Windows a `.bat`/`.cmd` program is run
/// through `cmd /C` (those files need a shell), matching pnpm's `shell`
/// handling.
pub fn run_token_helper_command(command: &[String]) -> io::Result<TokenHelperOutput> {
    run_token_helper_command_with_timeout(command, TOKEN_HELPER_TIMEOUT)
}

/// [`run_token_helper_command`] with an explicit deadline, so tests can
/// drive the timeout branch without waiting the full production bound.
fn run_token_helper_command_with_timeout(
    command: &[String],
    timeout: Duration,
) -> io::Result<TokenHelperOutput> {
    let Some((program, args)) = command.split_first() else {
        return Ok(TokenHelperOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        });
    };
    // Close stdin (a helper that reads it gets EOF instead of blocking)
    // and pipe stdout/stderr so the reader threads below can drain them.
    let mut child = build_command(program, args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Drain both pipes on their own threads: a helper that fills the
    // stdout (or stderr) buffer would otherwise block on the write while
    // the parent waits on the other pipe — a deadlock the poll loop can't
    // break.
    let stdout = read_pipe(child.stdout.take());
    let stderr = read_pipe(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "token helper exceeded its time limit",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };

    Ok(TokenHelperOutput {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Spawn a thread that reads `pipe` to end-of-stream as lossy UTF-8. A
/// read error or absent pipe yields the empty string.
fn read_pipe<Pipe: Read + Send + 'static>(pipe: Option<Pipe>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let Some(mut pipe) = pipe else { return String::new() };
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
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
