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
    let expanded_script = expand_braced_parameters(script, env);
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

/// Rewrite the POSIX `${...}` parameter expansions that the bundled shell
/// parser does not recognize into the `$NAME` references it does.
///
/// Only the script's own text is ever rewritten. A parameter that has a value
/// becomes a `$NAME` reference the shell expands itself, after parsing, so a
/// value holding shell punctuation cannot turn into a command, a redirection,
/// or a substitution. The `word` of a `${NAME:-word}` is kept exactly as the
/// script wrote it, quoting included.
///
/// Which side of a `${NAME:-word}` or `${NAME:+word}` is taken is read from
/// `env`, the environment the shell starts with, so a parameter that an
/// earlier command in the same script assigned is not seen. Forms no `$NAME`
/// reference can stand in for — `${NAME:=word}`, `${NAME:?word}`, `${#NAME}`,
/// `${NAME%suffix}` — are left verbatim.
fn expand_braced_parameters(script: &str, env: &HashMap<String, String>) -> String {
    let bytes = script.as_bytes();
    let mut expanded = String::with_capacity(script.len());
    let mut quote = None;
    let mut index = 0;

    while index < script.len() {
        let character = script[index..]
            .chars()
            .next()
            .expect("index sits on a character boundary");
        let next = bytes.get(index + 1).copied();
        index += character.len_utf8();

        match next_step(character, next, quote) {
            Step::Copy => expanded.push(character),
            Step::KeepEscapedDollar => {
                expanded.push_str(r"\$");
                index += 1;
            }
            Step::Quote(now_inside) => {
                quote = now_inside;
                expanded.push(character);
            }
            Step::Expand => {
                let start = index - 1;
                index =
                    push_braced_parameter(&mut expanded, script, start, env, quote == Some('"'));
            }
        }
    }

    expanded
}

/// What the character at the cursor does to the script being rewritten.
enum Step {
    /// Goes through as the text it is.
    Copy,
    /// Goes through together with the `$` it escapes, leaving the braces
    /// after it literal.
    KeepEscapedDollar,
    /// Goes through and leaves the rewrite inside the quote it names, or
    /// outside every quote once it closes one.
    Quote(Option<char>),
    /// Opens a `${...}` to rewrite.
    Expand,
}

/// Read `character` in the light of the quote it sits in. `next` is the byte
/// after it, which is what tells a backslash and a dollar sign apart from the
/// ordinary text they are everywhere else.
fn next_step(character: char, next: Option<u8>, quote: Option<char>) -> Step {
    // A single-quoted run is literal all the way to its own closing quote.
    if quote == Some('\'') {
        return if character == '\'' { Step::Quote(None) } else { Step::Copy };
    }

    match (character, next) {
        ('\\', Some(b'$')) => Step::KeepEscapedDollar,
        ('\'', _) if quote.is_none() => Step::Quote(Some('\'')),
        ('"', _) if quote.is_none() => Step::Quote(Some('"')),
        ('"', _) => Step::Quote(None),
        ('$', Some(b'{')) => Step::Expand,
        _ => Step::Copy,
    }
}

/// Append the replacement for the `${...}` that starts at `start` to
/// `expanded` and return the index just past what was consumed. A `${` the
/// script never closes, or one holding a form that has no `$NAME` equivalent,
/// is copied through as the text it already is.
fn push_braced_parameter(
    expanded: &mut String,
    script: &str,
    start: usize,
    env: &HashMap<String, String>,
    in_double_quotes: bool,
) -> usize {
    let Some(close) = braced_parameter_end(script, start, in_double_quotes) else {
        expanded.push('$');
        return start + 1;
    };
    let Some(replacement) = expand_parameter(&script[start + 2..close], env) else {
        expanded.push_str(&script[start..=close]);
        return close + 1;
    };

    expanded.push_str(&replacement);
    // `$NAME` swallows every name byte after it, so an expansion glued to more
    // of the same word is closed off with an empty string. The empty string
    // joins the word without contributing to it, quoted or not.
    if script
        .as_bytes()
        .get(close + 1)
        .copied()
        .is_some_and(is_name_byte)
    {
        expanded.push_str(r#""""#);
    }
    close + 1
}

/// The index of the `}` closing the `${` at `start`, or `None` when the script
/// has none. A brace inside quotes or behind a backslash is text, and a nested
/// `{` has to close before the expansion does. Within a double-quoted word an
/// apostrophe is an ordinary character, so `"${NAME:-it's fine}"` closes where
/// it looks like it does.
fn braced_parameter_end(script: &str, start: usize, in_double_quotes: bool) -> Option<usize> {
    let bytes = script.as_bytes();
    let opens_a_quote = |byte| byte == b'"' || (!in_double_quotes && byte == b'\'');
    let mut quote = None;
    let mut depth = 1_usize;
    let mut index = start + 2;

    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        match (quote, byte) {
            (Some(open), _) if open == byte => quote = None,
            (Some(_), _) => {}
            (None, b'\\') => index += 1,
            (None, _) if opens_a_quote(byte) => quote = Some(byte),
            (None, b'{') => depth += 1,
            (None, b'}') if depth == 1 => return Some(index - 1),
            (None, b'}') => depth -= 1,
            (None, _) => {}
        }
    }

    None
}

/// The replacement text for the body of a `${...}`, or `None` for a body that
/// no `$NAME` reference can stand in for.
fn expand_parameter(body: &str, env: &HashMap<String, String>) -> Option<String> {
    let name_length = body
        .bytes()
        .take_while(|byte| is_name_byte(*byte))
        .count();
    let (name, operator) = body.split_at(name_length);
    if name.is_empty() {
        return None;
    }
    if operator.is_empty() {
        return Some(format!("${name}"));
    }

    // A leading `:` makes an empty value count as unset, as POSIX specifies.
    let (operator, empty_counts_as_unset) = match operator.strip_prefix(':') {
        Some(rest) => (rest, true),
        None => (operator, false),
    };
    let word = operator.get(1..)?;
    let has_value = env
        .get(name)
        .is_some_and(|value| !empty_counts_as_unset || !value.is_empty());

    match (operator.as_bytes().first()?, has_value) {
        (b'-', true) => Some(format!("${name}")),
        (b'-', false) | (b'+', true) => Some(expand_braced_parameters(word, env)),
        (b'+', false) => Some(String::new()),
        _ => None,
    }
}

/// Whether the bundled shell parser reads `byte` as part of a `$NAME`.
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests;
