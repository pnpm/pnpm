use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState, SignalKind, execute_with_pipes,
    parser, parser::SequentialList, pipe,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_reporter::LifecycleStdio;
use std::{
    collections::HashMap,
    ffi::OsString,
    io::{self, Write},
    path::{self, Path, PathBuf},
    thread,
};
use tokio::{runtime::Builder, task::LocalSet};

use crate::process_tracker::{EmulatedCancellation, ProcessTracker};

/// Failure to run a script under the `shellEmulator` setting. A script
/// that runs and exits non-zero is not an error here — the exit code is
/// returned to the caller, which decides what a failure means.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ShellEmulatorError {
    #[display("Failed to parse `{script}` with the shell emulator: {message}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SHELL_EMULATOR_PARSE))]
    Parse { script: String, message: String },

    #[display("Failed to start the shell emulator for `{script}`: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SHELL_EMULATOR_START))]
    Start {
        script: String,
        #[error(source)]
        source: io::Error,
    },
}

/// Where an emulated script's output goes.
#[derive(Clone, Copy)]
pub enum EmulatedOutput<'a> {
    /// Straight to pacquet's own stdout and stderr, for a foreground
    /// `pnpm run`.
    Inherit,
    /// One call per output line, tagged with the stream it came from,
    /// for the install-time path that turns lines into reporter events.
    Lines(&'a (dyn Fn(LifecycleStdio, String) + Sync)),
}

/// Run `script` through the built-in shell instead of the platform's
/// own (`shellEmulator`), so a script written for `sh` behaves the same
/// on Windows. Returns the script's exit code.
///
/// `env` is the fully built script environment, `PATH` included; the
/// emulated shell resolves commands against it rather than against
/// pacquet's own environment.
pub fn execute_emulated(
    script: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
    output: EmulatedOutput<'_>,
    process_tracker: Option<&ProcessTracker>,
) -> Result<i32, ShellEmulatorError> {
    let expanded_script = expand_shell_emulator_vars(script, env);
    let list = parser::parse(&expanded_script).map_err(|error| ShellEmulatorError::Parse {
        script: script.to_string(),
        message: error.to_string(),
    })?;
    // `ShellState` requires an absolute cwd. Every production caller
    // already passes one; resolving here keeps a relative path from
    // reaching the panic inside the shell.
    let cwd = path::absolute(cwd)
        .map_err(|source| ShellEmulatorError::Start { script: script.to_string(), source })?;
    let cancellation = process_tracker.map(ProcessTracker::track_emulated);
    let run = EmulatedRun {
        list,
        env: env.iter().map(|(key, value)| (OsString::from(key), OsString::from(value))).collect(),
        cwd,
        cancellation: cancellation.as_ref().map(EmulatedCancellation::receiver),
    };
    match output {
        EmulatedOutput::Inherit => {
            run.complete(script, ShellPipeWriter::stdout(), ShellPipeWriter::stderr())
        }
        EmulatedOutput::Lines(sink) => run.complete_into_lines(script, sink),
    }
}

/// A parsed script with everything its run needs but its output pipes.
struct EmulatedRun {
    list: SequentialList,
    env: HashMap<OsString, OsString>,
    cwd: PathBuf,
    cancellation: Option<tokio::sync::watch::Receiver<bool>>,
}

impl EmulatedRun {
    /// Run with each output line handed to `sink`, tagged with its stream.
    fn complete_into_lines(
        self,
        script: &str,
        sink: &(dyn Fn(LifecycleStdio, String) + Sync),
    ) -> Result<i32, ShellEmulatorError> {
        thread::scope(|scope| {
            let (stdout_reader, stdout_writer) = pipe();
            let (stderr_reader, stderr_writer) = pipe();
            let stdout_pump =
                scope.spawn(move || pump_lines(stdout_reader, LifecycleStdio::Stdout, sink));
            let stderr_pump =
                scope.spawn(move || pump_lines(stderr_reader, LifecycleStdio::Stderr, sink));

            // Both writers are consumed by the run, so the pumps see EOF
            // as soon as it returns and the joins below finish promptly.
            let code = self.complete(script, stdout_writer, stderr_writer);
            let _ = stdout_pump.join();
            let _ = stderr_pump.join();
            code
        })
    }

    /// Drive the parsed script to completion and return its exit code.
    ///
    /// The shell is driven on a thread of our own because
    /// `deno_task_shell` needs a current-thread tokio runtime with a
    /// `LocalSet` (it uses `spawn_local`), and building one on the calling
    /// thread would panic whenever that thread is already inside a runtime.
    fn complete(
        self,
        script: &str,
        stdout: ShellPipeWriter,
        stderr: ShellPipeWriter,
    ) -> Result<i32, ShellEmulatorError> {
        let run = thread::spawn(move || self.execute(stdout, stderr));
        match run.join() {
            Ok(result) => result
                .map_err(|source| ShellEmulatorError::Start { script: script.to_string(), source }),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Execute on the current thread, with `deno_task_shell`'s own
    /// current-thread runtime and `LocalSet`.
    fn execute(self, stdout: ShellPipeWriter, stderr: ShellPipeWriter) -> io::Result<i32> {
        let EmulatedRun { list, env, cwd, cancellation } = self;
        let runtime = Builder::new_current_thread().enable_all().build()?;
        let kill_signal = KillSignal::default();
        let local_set = LocalSet::new();
        if let Some(mut cancellation) = cancellation {
            let cancellation_signal = kill_signal.clone();
            local_set.spawn_local(async move {
                if *cancellation.borrow_and_update() || cancellation.changed().await.is_ok() {
                    cancellation_signal.send(SignalKind::SIGKILL);
                }
            });
        }
        let state = ShellState::new(env, cwd, HashMap::new(), kill_signal);
        let stdin = ShellPipeReader::stdin();
        Ok(local_set.block_on(&runtime, execute_with_pipes(list, state, stdin, stdout, stderr)))
    }
}

/// Read `reader` to EOF, handing each line to `sink`.
fn pump_lines(
    reader: ShellPipeReader,
    stdio: LifecycleStdio,
    sink: &(dyn Fn(LifecycleStdio, String) + Sync),
) {
    let mut writer = LineWriter { stdio, sink, pending: Vec::new() };
    let _ = reader.pipe_to(&mut writer);
    writer.flush_pending();
}

/// Splits the chunks a [`ShellPipeReader`] writes into lines, holding a
/// partial trailing line until the chunk that completes it arrives.
struct LineWriter<'a> {
    stdio: LifecycleStdio,
    sink: &'a (dyn Fn(LifecycleStdio, String) + Sync),
    pending: Vec<u8>,
}

impl LineWriter<'_> {
    /// Emit whatever is left after EOF: a final line with no trailing
    /// newline. A stream that ended on a newline leaves nothing here.
    fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            (self.sink)(self.stdio, String::from_utf8_lossy(&self.pending).into_owned());
            self.pending.clear();
        }
    }
}

impl Write for LineWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        while let Some(end) = self.pending.iter().position(|&byte| byte == b'\n') {
            let mut line = self.pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            (self.sink)(self.stdio, String::from_utf8_lossy(&line).into_owned());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Expands POSIX shell `${VAR}` parameter expansions (including default values like `${VAR:-default}`)
/// in `script` using the provided `env` map, respecting single-quote literal boundaries and backslash escapes.
#[must_use]
pub fn expand_shell_emulator_vars(script: &str, env: &HashMap<String, String>) -> String {
    let bytes = script.as_bytes();
    let len = bytes.len();
    let mut output = String::with_capacity(len);
    let mut i = 0;

    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < len {
        let b = bytes[i];

        if b == b'\\' && !in_single_quote {
            let bs_start = i;
            while i < len && bytes[i] == b'\\' {
                i += 1;
            }
            let bs_count = i - bs_start;

            if i < len && bytes[i] == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                for _ in 0..(bs_count / 2) {
                    output.push('\\');
                }
                if bs_count % 2 == 1 {
                    output.push('$');
                    output.push('{');
                    i += 2;
                    if let Some((end_idx, _)) = find_braced_var_end(script, i - 2) {
                        output.push_str(&script[i..=end_idx]);
                        i = end_idx + 1;
                    }
                    continue;
                }
            } else {
                output.push_str(&script[bs_start..i]);
                continue;
            }
        }

        if b == b'\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            output.push('\'');
            i += 1;
            continue;
        }

        if b == b'"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            output.push('"');
            i += 1;
            continue;
        }

        if b == b'$'
            && !in_single_quote
            && i + 1 < len
            && bytes[i + 1] == b'{'
            && let Some((end_idx, var_expr)) = find_braced_var_end(script, i)
        {
            let expanded_val = evaluate_parameter_expansion(var_expr, env);
            output.push_str(&expanded_val);
            i = end_idx + 1;
            continue;
        }

        if let Some(ch) = script[i..].chars().next() {
            output.push(ch);
            i += ch.len_utf8();
        } else {
            i += 1;
        }
    }

    output
}

fn find_braced_var_end(script: &str, start_idx: usize) -> Option<(usize, &str)> {
    let bytes = script.as_bytes();
    if start_idx + 1 >= bytes.len() || bytes[start_idx] != b'$' || bytes[start_idx + 1] != b'{' {
        return None;
    }
    let body_start = start_idx + 2;
    let mut cursor = body_start;
    let mut depth = 1;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((cursor, &script[body_start..cursor]));
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn evaluate_parameter_expansion(expr: &str, env: &HashMap<String, String>) -> String {
    let expr =
        if expr.contains("${") { expand_shell_emulator_vars(expr, env) } else { expr.to_string() };
    let expr_str = expr.as_str();

    if let Some(pos) = expr_str.find(":-") {
        let var_name = &expr_str[..pos];
        let default_val = &expr_str[pos + 2..];
        let val = env.get(var_name).filter(|v| !v.is_empty());
        match val {
            Some(v) => v.clone(),
            None => default_val.to_string(),
        }
    } else if let Some(pos) = expr_str.find(":+") {
        let var_name = &expr_str[..pos];
        let alt_val = &expr_str[pos + 2..];
        let val = env.get(var_name).filter(|v| !v.is_empty());
        match val {
            Some(_) => alt_val.to_string(),
            None => String::new(),
        }
    } else if let Some(pos) = expr_str.find(":=") {
        let var_name = &expr_str[..pos];
        let default_val = &expr_str[pos + 2..];
        let val = env.get(var_name).filter(|v| !v.is_empty());
        match val {
            Some(v) => v.clone(),
            None => default_val.to_string(),
        }
    } else if let Some(pos) = expr_str.find('-') {
        let var_name = &expr_str[..pos];
        let default_val = &expr_str[pos + 1..];
        match env.get(var_name) {
            Some(v) => v.clone(),
            None => default_val.to_string(),
        }
    } else if let Some(pos) = expr_str.find('+') {
        let var_name = &expr_str[..pos];
        let alt_val = &expr_str[pos + 1..];
        match env.get(var_name) {
            Some(_) => alt_val.to_string(),
            None => String::new(),
        }
    } else if let Some(pos) = expr_str.find('=') {
        let var_name = &expr_str[..pos];
        let default_val = &expr_str[pos + 1..];
        match env.get(var_name) {
            Some(v) => v.clone(),
            None => default_val.to_string(),
        }
    } else {
        env.get(expr_str).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
