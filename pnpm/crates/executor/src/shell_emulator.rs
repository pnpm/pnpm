pub use crate::shell_emulator_types::{EmulatedOutput, ShellEmulatorError};

use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState, SignalKind, execute_with_pipes,
    parser, parser::SequentialList, pipe,
};
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
    let expanded_script = braced_parameters::expand(script, env);
    let list = parser::parse(&expanded_script)
        .map_err(|error| ShellEmulatorError::Parse {
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
        env: env
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect(),
        cwd,
        cancellation: cancellation.as_ref().map(EmulatedCancellation::to_receiver),
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
            Ok(result) => result.map_err(|source| ShellEmulatorError::Start {
                script: script.to_string(),
                source,
            }),
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
        while let Some(end) = self.pending
            .iter()
            .position(|&byte| byte == b'\n')
        {
            let mut line = self.pending
                .drain(..=end)
                .collect::<Vec<_>>();
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

/// Rewriting `${...}` parameter expansions into the `$NAME` references the
/// bundled parser understands.
mod braced_parameters;

#[cfg(test)]
mod tests;
