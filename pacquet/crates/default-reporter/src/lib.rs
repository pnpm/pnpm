//! pnpm-identical visual reporter for pacquet.
//!
//! [`DefaultReporter`] is a [`pacquet_reporter::Reporter`] sink that renders
//! the same terminal output `@pnpm/cli.default-reporter` produces for
//! `install` / `add` / `update` / `remove`: a live progress line, a
//! packages-diff summary, lifecycle script output, and a `Done in ...` footer.
//!
//! The trait's `emit` is a static method, so all state lives behind a
//! process-global mutex. Each event is folded into a [`ReporterState`] (see
//! [`state`]) that recomputes the frame; the sink writes it in place (TTY) or
//! appends it line by line (non-TTY).
//!
//! Two values the renderer can't recover from events are injected once at
//! startup: [`set_cwd`] (the project root, for relative paths and workspace
//! zooming) and [`set_package_version`] (rendered in the `Done in ...` line).

pub mod colors;
mod diff;
pub mod format;
pub mod state;

use std::{
    io::{IsTerminal, Write},
    sync::{LazyLock, Mutex, OnceLock},
    time::{Duration, Instant},
};

use pacquet_reporter::{FetchingProgressMessage, LogEvent, Reporter};

use crate::{
    colors::Colors,
    state::{Output, ReporterState},
};

static CWD: OnceLock<String> = OnceLock::new();
static PACKAGE_VERSION: OnceLock<String> = OnceLock::new();
static FORCE_APPEND_ONLY: OnceLock<bool> = OnceLock::new();
static FILTER_SUMMARY_BY_PREFIX: OnceLock<bool> = OnceLock::new();

/// Set the project root the reporter renders paths relative to. Call once
/// before the first event; ignored if already set.
pub fn set_cwd(cwd: impl Into<String>) {
    let _ = CWD.set(cwd.into());
}

/// Set the version rendered in the `Done in ... using pnpm v<version>`
/// footer. Call once before the first event; ignored if already set.
pub fn set_package_version(version: impl Into<String>) {
    let _ = PACKAGE_VERSION.set(version.into());
}

pub(crate) fn package_version() -> &'static str {
    PACKAGE_VERSION.get().map(String::as_str).unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Force append-only rendering regardless of whether stdout is a TTY,
/// backing `--reporter=append-only`. Call once before the first event.
pub fn force_append_only() {
    let _ = FORCE_APPEND_ONLY.set(true);
}

/// Configure whether the packages-diff summary is restricted to `cwd`.
/// Global installs aggregate every group, matching pnpm's global reporter path.
pub fn set_filter_summary_by_prefix(filter: bool) {
    let _ = FILTER_SUMMARY_BY_PREFIX.set(filter);
}

fn cwd() -> String {
    CWD.get().cloned().unwrap_or_else(|| {
        std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
    })
}

/// `--reporter=default`: renders pnpm-style visual output to stdout.
pub struct DefaultReporter;

impl Reporter for DefaultReporter {
    fn emit(event: &LogEvent) {
        let mut sink = SINK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = sink.state.handle(event);
        sink.write(output, is_coalesceable(event));
    }
}

/// Whether an event is a high-volume progress update that may be dropped
/// under throttling, mirroring pnpm's `throttleProgress` on the progress
/// stream.
fn is_coalesceable(event: &LogEvent) -> bool {
    match event {
        LogEvent::Progress(_) => true,
        LogEvent::FetchingProgress(log) => {
            matches!(log.message, FetchingProgressMessage::InProgress { .. })
        }
        _ => false,
    }
}

static SINK: LazyLock<Mutex<Sink>> = LazyLock::new(|| Mutex::new(Sink::new()));

struct Sink {
    state: ReporterState,
    diff: diff::Diff,
    /// Reused across frames so the hot progress path composes the whole
    /// redraw without allocating, and writes it as a single `write_all`
    /// (an atomic update other writers can't interleave into).
    frame_buf: String,
    throttle: Duration,
    last_write: Option<Instant>,
}

impl Sink {
    fn new() -> Self {
        let is_tty = std::io::stdout().is_terminal();
        let append_only = !is_tty || FORCE_APPEND_ONLY.get().copied().unwrap_or(false);
        let columns = if is_tty { terminal_columns().unwrap_or(80) } else { 80 };
        // pnpm's `outputMaxWidth`: `columns - 2` on a TTY, else 80.
        let width = if is_tty { columns.saturating_sub(2) } else { 80 };
        let colors = Colors { enabled: is_tty && std::env::var_os("NO_COLOR").is_none() };
        let state = ReporterState::new_with_summary_prefix_filter(
            cwd(),
            width,
            colors,
            append_only,
            FILTER_SUMMARY_BY_PREFIX.get().copied().unwrap_or(true),
        );
        let diff = diff::Diff::new(columns);
        let throttle =
            if append_only { Duration::from_secs(1) } else { Duration::from_millis(200) };
        Sink { state, diff, frame_buf: String::new(), throttle, last_write: None }
    }

    fn write(&mut self, output: Output, coalesceable: bool) {
        // Drop a high-volume progress redraw if the throttle window hasn't
        // elapsed. State is already folded, so the next non-coalesceable
        // event (stats, summary, importing-done, the footer) renders the
        // latest counts.
        if coalesceable && self.last_write.is_some_and(|last| last.elapsed() < self.throttle) {
            return;
        }
        let wrote = self.write_output(output);
        if wrote {
            self.last_write = Some(Instant::now());
        }
    }

    /// Returns whether anything was written.
    fn write_output(&mut self, output: Output) -> bool {
        let mut out = std::io::stdout().lock();
        match output {
            Output::None => return false,
            Output::Lines(lines) => {
                for line in lines {
                    let _ = writeln!(out, "{line}");
                }
            }
            Output::Frame(mut frame) => {
                // A trailing newline keeps an interactive prompt on a fresh line
                // below the frame rather than joined onto its last line, and it
                // leaves the differ's tracked cursor at column 0 so it stays in
                // sync with the `\r` prepended on the next update (otherwise the
                // inline diff computes relative moves from a stale column).
                if !frame.ends_with('\n') {
                    frame.push('\n');
                }
                // `\r` resets the column in case an external process left the
                // cursor mid-line; `\x1b[K` erases trailing characters on the
                // current line; `\x1b[0J` erases anything written below the
                // rendered frame.
                self.frame_buf.clear();
                self.frame_buf.push('\r');
                self.diff.update_into(&frame, &mut self.frame_buf);
                self.frame_buf.push_str("\x1b[K\x1b[0J");
                let _ = out.write_all(self.frame_buf.as_bytes());
            }
        }
        let _ = out.flush();
        true
    }
}

#[cfg(unix)]
fn terminal_columns() -> Option<usize> {
    // SAFETY: `winsize` is plain-old-data; `ioctl` only writes into it and we
    // check the return code before reading.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0)
            .then_some(ws.ws_col as usize)
    }
}

#[cfg(not(unix))]
fn terminal_columns() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests;
