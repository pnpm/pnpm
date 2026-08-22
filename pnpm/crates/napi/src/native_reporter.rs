//! Rendering pnpm's terminal output inside the engine.
//!
//! Without this, an embedder gets the raw event stream through `onLog` and
//! has to render it itself — in practice by keeping `@pnpm/logger` and
//! `@pnpm/cli.default-reporter` around and feeding the events into them.
//! That works only for as long as the JS reporter of one pnpm line stays
//! wire-compatible with the events of another, which is a coupling an
//! embedder should not have to maintain. [`NativeRenderer`] folds the same
//! events through [`pnpm_default_reporter`] — the reporter `pnpm install`
//! itself renders with — so an embedder gets pnpm's real output and no JS
//! reporter.
//!
//! Output goes to stdout (or stderr) by default. An embedder that owns the
//! terminal — Bit's CLI server replaces `process.stdout` with a stream that
//! forwards to connected editors — passes an `onOutput` callback instead,
//! and every rendered chunk is handed back to JS rather than written to a
//! file descriptor the host has already redirected at the JS level.

use std::{
    io::{IsTerminal, Write},
    time::{Duration, Instant},
};

use napi::{
    Status,
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode, UnknownReturnValue},
};
use napi_derive::napi;
use pnpm_default_reporter::{
    MaxLogLevel,
    colors::Colors,
    diff::Diff,
    state::{Output, ReporterOptions as StateOptions, ReporterState},
};
use pnpm_reporter::{FetchingProgressMessage, LogEvent};

/// A JS `(chunk: string) => void` callback. Same shape as the log sink:
/// `CalleeHandled = false`, return value discarded, never blocking.
pub type OutputSink = ThreadsafeFunction<String, UnknownReturnValue, String, Status, false>;

/// pnpm's default terminal output, rendered by the engine. Mirrors
/// [`ReporterOptions`] in `index.d.ts`; every field maps onto the option of
/// the same name in `@pnpm/cli.default-reporter`'s `reportingOptions`.
#[napi(object)]
#[derive(Default)]
pub struct ReporterOptions {
    /// Print each update on its own line instead of redrawing the frame in
    /// place. The right choice whenever the output is not a live terminal.
    pub append_only: Option<bool>,
    /// Milliseconds between progress redraws. Defaults to 1000 in
    /// append-only mode and 200 otherwise, as pnpm does.
    pub throttle_progress: Option<u32>,
    /// Leave the materialized-package count out of the progress line.
    pub hide_added_pkgs_progress: Option<bool>,
    /// Leave the workspace-project prefix out of progress lines.
    pub hide_progress_prefix: Option<bool>,
    /// Keep dependency build-script output in its collapsed block instead
    /// of streaming every line.
    pub hide_lifecycle_output: Option<bool>,
    /// Replaces the `Run "pnpm approve-builds" ...` line under the list of
    /// packages whose build scripts were blocked. For an embedder whose
    /// users approve builds through its own configuration.
    pub ignored_builds_instruction_text: Option<String>,
    /// Package-name patterns whose linked (symlinked-in) entries are left
    /// out of the packages-diff summary. An embedder that links its own
    /// runtime into every project silences that noise without silencing
    /// the same packages when they are really installed.
    pub hide_linked_pkgs_diff: Option<Vec<String>>,
    /// `"error"`, `"warn"`, `"info"` (the default), or `"debug"`.
    pub log_level: Option<String>,
    /// Terminal width to wrap at, at least one column. Defaults to the
    /// width of the output stream when it is a terminal, and 80 otherwise —
    /// an `onOutput` callback always needs it passed explicitly, since the
    /// engine cannot see where the chunks end up.
    pub width: Option<u32>,
    /// Whether to emit ANSI color. Defaults to "the output stream is a
    /// terminal and `NO_COLOR` is unset"; with an `onOutput` callback, to
    /// `false`.
    pub color: Option<bool>,
    /// Render output on stderr rather than stdout. Ignored when an
    /// `onOutput` callback is given.
    pub use_stderr: Option<bool>,
    /// Directory paths are rendered relative to. Defaults to the
    /// operation's `dir`.
    pub cwd: Option<String>,
}

/// Where a rendered chunk goes.
enum Destination {
    Stdout,
    Stderr,
    /// The host's `onOutput`. Enqueued non-blocking, in order; a closed or
    /// saturated queue drops the chunk rather than blocking an engine
    /// worker thread, exactly like the log sink.
    Callback(OutputSink),
    /// Collects the chunks in memory. Test-only: a [`ThreadsafeFunction`]
    /// needs a live napi environment, which the unit-test binary has none
    /// of.
    #[cfg(test)]
    Buffer(std::sync::Arc<std::sync::Mutex<String>>),
}

impl Destination {
    fn write(&self, chunk: &str) {
        match self {
            Destination::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(chunk.as_bytes());
                let _ = out.flush();
            }
            Destination::Stderr => {
                let mut out = std::io::stderr().lock();
                let _ = out.write_all(chunk.as_bytes());
                let _ = out.flush();
            }
            Destination::Callback(sink) => {
                sink.call(chunk.to_string(), ThreadsafeFunctionCallMode::NonBlocking);
            }
            #[cfg(test)]
            Destination::Buffer(buffer) => {
                buffer.lock().expect("the test buffer is never poisoned").push_str(chunk);
            }
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Destination::Stdout => std::io::stdout().is_terminal(),
            Destination::Stderr => std::io::stderr().is_terminal(),
            // The engine cannot see what the host does with the chunks.
            Destination::Callback(_) => false,
            #[cfg(test)]
            Destination::Buffer(_) => false,
        }
    }

    /// The terminal width of *this* destination. Measuring stdout while
    /// rendering to stderr would size the frame from an unrelated stream —
    /// they are separately redirectable and need not be the same width.
    fn terminal_columns(&self) -> Option<usize> {
        match self {
            Destination::Stdout => terminal_columns(StreamFd::Stdout),
            Destination::Stderr => terminal_columns(StreamFd::Stderr),
            Destination::Callback(_) => None,
            #[cfg(test)]
            Destination::Buffer(_) => None,
        }
    }
}

/// One engine call's renderer: the folded reporter state plus the frame
/// differ and throttle that turn it into terminal writes. A fresh one is
/// built per call, so consecutive installs in one process do not inherit
/// each other's counters or options — unlike the CLI's process-global
/// reporter, which is configured once at startup.
pub struct NativeRenderer {
    state: ReporterState,
    diff: Diff,
    /// Reused across frames so the hot progress path composes a redraw
    /// without allocating and writes it as one chunk.
    frame_buf: String,
    throttle: Duration,
    last_write: Option<Instant>,
    destination: Destination,
}

impl NativeRenderer {
    /// Build the renderer for one engine call. `dir` is the operation's
    /// directory, used as the render root when the options name none.
    pub fn new(options: &ReporterOptions, dir: &str, on_output: Option<OutputSink>) -> Self {
        let destination = match on_output {
            Some(sink) => Destination::Callback(sink),
            None if options.use_stderr.unwrap_or(false) => Destination::Stderr,
            None => Destination::Stdout,
        };
        Self::with_destination(options, dir, destination)
    }

    fn with_destination(options: &ReporterOptions, dir: &str, destination: Destination) -> Self {
        let is_terminal = destination.is_terminal();
        let append_only = options.append_only.unwrap_or(!is_terminal);
        // pnpm's `outputMaxWidth`: the terminal's columns less 2, or 80.
        // Floored at one column, so a host that computed its width the same
        // way from a one- or two-column terminal cannot ask the renderer to
        // wrap at zero.
        let width = options
            .width
            .map_or_else(
                || {
                    if is_terminal {
                        destination.terminal_columns().unwrap_or(82).saturating_sub(2)
                    } else {
                        80
                    }
                },
                |width| width as usize,
            )
            .max(1);
        let colors = Colors {
            enabled: options
                .color
                .unwrap_or_else(|| is_terminal && std::env::var_os("NO_COLOR").is_none()),
        };
        let state = ReporterState::new_with_options(
            options.cwd.clone().unwrap_or_else(|| dir.to_string()),
            width,
            colors,
            StateOptions {
                append_only,
                hide_added_pkgs_progress: options.hide_added_pkgs_progress.unwrap_or(false),
                hide_progress_prefix: options.hide_progress_prefix.unwrap_or(false),
                hide_lifecycle_output: options.hide_lifecycle_output.unwrap_or(false),
                ignored_builds_instruction_text: options.ignored_builds_instruction_text.clone(),
                hide_linked_pkgs_diff: options.hide_linked_pkgs_diff.clone().unwrap_or_default(),
                max_log_level: parse_log_level(options.log_level.as_deref()),
                ..StateOptions::default()
            },
        );
        let throttle = options.throttle_progress.map_or(
            if append_only { Duration::from_secs(1) } else { Duration::from_millis(200) },
            |ms| Duration::from_millis(u64::from(ms)),
        );
        NativeRenderer {
            state,
            diff: Diff::new(width.saturating_add(2)),
            frame_buf: String::new(),
            throttle,
            last_write: None,
            destination,
        }
    }

    /// Fold one event and write whatever it produced.
    ///
    /// A prompt event is folded like any other: the binding decides build
    /// approval from its `allowBuilds` option and never prompts, so there
    /// is no interactive frame to hold back for.
    pub fn handle(&mut self, event: &LogEvent) {
        let output = self.state.handle(event);
        // Drop a high-volume progress redraw inside the throttle window.
        // The state is already folded, so the next event that is not
        // coalesceable (stats, the summary, the footer) renders the
        // current counts.
        if is_coalesceable(event)
            && self.last_write.is_some_and(|last| last.elapsed() < self.throttle)
        {
            return;
        }
        if self.write(output) {
            self.last_write = Some(Instant::now());
        }
    }

    /// Returns whether anything was written.
    fn write(&mut self, output: Output) -> bool {
        match output {
            Output::None => return false,
            Output::Lines(lines) => {
                let mut chunk = String::new();
                for line in lines {
                    chunk.push_str(&line);
                    chunk.push('\n');
                }
                self.destination.write(&chunk);
            }
            Output::Frame(mut frame) => {
                // A trailing newline leaves the differ's tracked cursor at
                // column 0, in sync with the `\r` prepended on the next
                // update.
                if !frame.ends_with('\n') {
                    frame.push('\n');
                }
                // `\r` resets the column in case another writer left the
                // cursor mid-line; `\x1b[K` erases the rest of the current
                // line and `\x1b[0J` everything below the frame.
                self.frame_buf.clear();
                self.frame_buf.push('\r');
                self.diff.update_into(&frame, &mut self.frame_buf);
                self.frame_buf.push_str("\x1b[K\x1b[0J");
                let chunk = std::mem::take(&mut self.frame_buf);
                self.destination.write(&chunk);
                self.frame_buf = chunk;
            }
        }
        true
    }
}

/// Whether an event is a high-volume progress update that may be dropped
/// under throttling, mirroring pnpm's `throttleProgress`.
fn is_coalesceable(event: &LogEvent) -> bool {
    match event {
        LogEvent::Progress(_) => true,
        LogEvent::FetchingProgress(log) => {
            matches!(log.message, FetchingProgressMessage::InProgress { .. })
        }
        _ => false,
    }
}

/// An unrecognized level falls back to pnpm's own default rather than
/// failing the install: a reporting setting must never be what stops an
/// install from running.
fn parse_log_level(level: Option<&str>) -> MaxLogLevel {
    match level {
        Some("error") => MaxLogLevel::Error,
        Some("warn") => MaxLogLevel::Warn,
        Some("debug") => MaxLogLevel::Debug,
        _ => MaxLogLevel::Info,
    }
}

/// Which standard stream a width query is about.
#[derive(Clone, Copy)]
enum StreamFd {
    Stdout,
    Stderr,
}

#[cfg(unix)]
fn terminal_columns(stream: StreamFd) -> Option<usize> {
    let fd = match stream {
        StreamFd::Stdout => libc::STDOUT_FILENO,
        StreamFd::Stderr => libc::STDERR_FILENO,
    };
    // SAFETY: `winsize` is plain-old-data; `ioctl` only writes into it and
    // the return code is checked before it is read.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0)
            .then_some(ws.ws_col as usize)
    }
}

#[cfg(not(unix))]
fn terminal_columns(_stream: StreamFd) -> Option<usize> {
    None
}

#[cfg(test)]
mod tests;
