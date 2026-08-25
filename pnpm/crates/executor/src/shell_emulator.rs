use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState, execute_with_pipes, parser,
    parser::SequentialList, pipe,
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
) -> Result<i32, ShellEmulatorError> {
    let list = parser::parse(script).map_err(|error| ShellEmulatorError::Parse {
        script: script.to_string(),
        message: error.to_string(),
    })?;
    // `ShellState` requires an absolute cwd. Every production caller
    // already passes one; resolving here keeps a relative path from
    // reaching the panic inside the shell.
    let cwd = path::absolute(cwd)
        .map_err(|source| ShellEmulatorError::Start { script: script.to_string(), source })?;
    let env = env.iter().map(|(key, value)| (OsString::from(key), OsString::from(value))).collect();

    match output {
        EmulatedOutput::Inherit => run_to_completion(
            script,
            list,
            env,
            cwd,
            ShellPipeWriter::stdout(),
            ShellPipeWriter::stderr(),
        ),
        EmulatedOutput::Lines(sink) => thread::scope(|scope| {
            let (stdout_reader, stdout_writer) = pipe();
            let (stderr_reader, stderr_writer) = pipe();
            let stdout_pump =
                scope.spawn(move || pump_lines(stdout_reader, LifecycleStdio::Stdout, sink));
            let stderr_pump =
                scope.spawn(move || pump_lines(stderr_reader, LifecycleStdio::Stderr, sink));

            // Both writers are consumed by the run, so the pumps see EOF
            // as soon as it returns and the joins below finish promptly.
            let code = run_to_completion(script, list, env, cwd, stdout_writer, stderr_writer);
            let _ = stdout_pump.join();
            let _ = stderr_pump.join();
            code
        }),
    }
}

/// Drive the parsed script to completion and return its exit code.
///
/// The shell is driven on a thread of our own because
/// `deno_task_shell` needs a current-thread tokio runtime with a
/// `LocalSet` (it uses `spawn_local`), and building one on the calling
/// thread would panic whenever that thread is already inside a runtime.
fn run_to_completion(
    script: &str,
    list: SequentialList,
    env: HashMap<OsString, OsString>,
    cwd: PathBuf,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> Result<i32, ShellEmulatorError> {
    let run = thread::spawn(move || {
        let runtime = Builder::new_current_thread().enable_all().build()?;
        let state = ShellState::new(env, cwd, HashMap::new(), KillSignal::default());
        let stdin = ShellPipeReader::stdin();
        Ok(LocalSet::new()
            .block_on(&runtime, execute_with_pipes(list, state, stdin, stdout, stderr)))
    });

    match run.join() {
        Ok(result) => result
            .map_err(|source| ShellEmulatorError::Start { script: script.to_string(), source }),
        Err(payload) => std::panic::resume_unwind(payload),
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

#[cfg(test)]
mod tests;
