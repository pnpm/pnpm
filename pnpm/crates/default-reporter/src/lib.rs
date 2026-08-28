//! pnpm-identical visual reporter for pacquet.
//!
//! [`DefaultReporter`] is a [`pnpm_reporter::Reporter`] sink that renders
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
pub mod diff;
pub mod format;
pub mod state;

use std::{
    io::{IsTerminal, Write},
    sync::{LazyLock, Mutex, OnceLock},
    time::{Duration, Instant},
};

use console::Term;
use pnpm_config::ColorMode;
use pnpm_reporter::{FetchingProgressMessage, LogEvent, PromptAction, Reporter};

use crate::{
    colors::Colors,
    format::visible_width,
    state::{Output, ReporterState},
};

static CWD: OnceLock<String> = OnceLock::new();
static USE_STDERR: OnceLock<bool> = OnceLock::new();
static PACKAGE_VERSION: OnceLock<String> = OnceLock::new();
static FORCE_APPEND_ONLY: OnceLock<bool> = OnceLock::new();
static SUMMARY_SCOPE: OnceLock<SummaryScope> = OnceLock::new();
static REPORTS_SCOPE: OnceLock<bool> = OnceLock::new();
static HIDE_ADDED_PKGS_PROGRESS: OnceLock<bool> = OnceLock::new();
static STREAM_LIFECYCLE_OUTPUT: OnceLock<bool> = OnceLock::new();
static AGGREGATE_OUTPUT: OnceLock<bool> = OnceLock::new();
static HIDE_LIFECYCLE_PREFIX: OnceLock<bool> = OnceLock::new();
static IS_RECURSIVE: OnceLock<bool> = OnceLock::new();
static MAX_LOG_LEVEL: OnceLock<MaxLogLevel> = OnceLock::new();
static COLOR_MODE: OnceLock<ColorMode> = OnceLock::new();

/// Verbosity ceiling for the rendered output, from pnpm's `--loglevel`
/// setting. Mirrors `LOG_LEVEL_NUMBER` in `@pnpm/cli.default-reporter`
/// (`error` = 0 ... `debug` = 3): a stream renders when its own tier is at
/// or below the ceiling, so the derived order makes `Error` the quietest
/// and `Debug` the loudest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaxLogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Which prefixes contribute to the packages-diff summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryScope {
    /// Include only events whose `prefix` is the configured current working directory.
    CurrentPrefix,
    /// Include events from every prefix, used by global install groups.
    AllPrefixes,
}

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

/// Route all reporter output (warnings, progress, the summary) to stderr
/// instead of stdout — pnpm's `useStderr`, set for the commands in its
/// `COMMANDS_WITH_STDERR_REPORTER` whose stdout is a machine-readable
/// value. TTY detection and terminal width follow the selected stream.
/// Call once before the first event.
pub fn use_stderr() {
    let _ = USE_STDERR.set(true);
}

fn is_stderr_output() -> bool {
    USE_STDERR.get().copied().unwrap_or(false)
}

/// Configure which prefixes contribute to the packages-diff summary.
pub fn set_summary_scope(scope: SummaryScope) {
    let _ = SUMMARY_SCOPE.set(scope);
}

/// Declare that the running command reports the workspace scope it
/// selected — pnpm's `COMMANDS_THAT_REPORT_SCOPE` gate. Call once before
/// the first event; ignored if already set.
pub fn set_reports_scope(reports_scope: bool) {
    let _ = REPORTS_SCOPE.set(reports_scope);
}

/// Configure whether dependency progress includes the materialization count.
///
/// This must be called before the reporter is initialized. Only the first
/// configured value is retained.
pub fn set_hide_added_pkgs_progress(hide_added_pkgs_progress: bool) {
    let _ = HIDE_ADDED_PKGS_PROGRESS.set(hide_added_pkgs_progress);
}

/// Stream lifecycle script output line by line instead of collecting it
/// into a collapsible block — pnpm's `--stream`. Call once before the
/// first event.
pub fn stream_lifecycle_output() {
    let _ = STREAM_LIFECYCLE_OUTPUT.set(true);
}

/// Hold each script's streamed output until it exits, then print the run
/// as one block — pnpm's `--aggregate-output`. Call once before the first
/// event.
pub fn aggregate_output() {
    let _ = AGGREGATE_OUTPUT.set(true);
}

/// Drop the project prefix from streamed script output lines — pnpm's
/// `--reporter-hide-prefix`. Call once before the first event.
pub fn hide_lifecycle_prefix() {
    let _ = HIDE_LIFECYCLE_PREFIX.set(true);
}

/// Configure whether the running command operates recursively.
///
/// This must be called before the reporter is initialized. Only the first
/// configured value is retained.
pub fn set_is_recursive(is_recursive: bool) {
    let _ = IS_RECURSIVE.set(is_recursive);
}

/// Configure the verbosity ceiling, backing pnpm's `--loglevel` option.
/// Call once before the first event; ignored if already set. Defaults to
/// [`MaxLogLevel::Info`], pnpm's fallback when no `loglevel` is given.
pub fn set_max_log_level(level: MaxLogLevel) {
    let _ = MAX_LOG_LEVEL.set(level);
}

/// Configure ANSI color rendering. Call before the first reporter event.
pub fn set_color_mode(mode: ColorMode) {
    let _ = COLOR_MODE.set(mode);
}

pub fn colors_enabled(is_terminal: bool) -> bool {
    match COLOR_MODE.get().copied().unwrap_or_default() {
        ColorMode::Always => true,
        ColorMode::Auto => is_terminal && std::env::var_os("NO_COLOR").is_none(),
        ColorMode::Never => false,
    }
}

fn cwd() -> String {
    CWD.get().cloned().unwrap_or_else(|| {
        std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
    })
}

/// `--reporter=default`: renders pnpm-style visual output to stdout, or to
/// stderr when [`use_stderr`] was configured.
pub struct DefaultReporter;

impl Reporter for DefaultReporter {
    fn emit(event: &LogEvent) {
        let mut sink = SINK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let LogEvent::Prompt(log) = event {
            sink.on_prompt(log.action);
            return;
        }
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
    /// The differ's frame width, and the terminal height the frame has to fit
    /// into. Re-read from [`terminal_size`] on every frame so a window resize
    /// takes effect. See [`Sink::commit_overflow`].
    columns: usize,
    rows: Option<usize>,
    committed_lines: usize,
    /// How many rows the frame the differ is holding takes up, and whether it
    /// already outgrew the terminal it was drawn on.
    rendered_frame_rows: usize,
    rendered_frame_outgrew_terminal: bool,
    /// Probe for the output terminal's dimensions. A field so a test can pin
    /// them instead of measuring the terminal the test runner happens to
    /// inherit.
    terminal_size: fn() -> Option<(usize, Option<usize>)>,
    throttle: Duration,
    last_write: Option<Instant>,
    prompt_active: bool,
    prompt_lines: Vec<String>,
    prompt_frame: Option<String>,
}

impl Sink {
    fn new() -> Self {
        let is_tty = if is_stderr_output() {
            std::io::stderr().is_terminal()
        } else {
            std::io::stdout().is_terminal()
        };
        let append_only = !is_tty || FORCE_APPEND_ONLY.get().copied().unwrap_or(false);
        let (columns, rows) =
            if is_tty { terminal_size().unwrap_or((80, None)) } else { (80, None) };
        // pnpm's `outputMaxWidth`: `columns - 2` on a TTY, else 80.
        let width = if is_tty { columns.saturating_sub(2) } else { 80 };
        let colors = Colors { enabled: colors_enabled(is_tty) };
        let state = ReporterState::new_with_options(
            cwd(),
            width,
            colors,
            state::ReporterOptions {
                append_only,
                summary_scope: SUMMARY_SCOPE.get().copied().unwrap_or(SummaryScope::CurrentPrefix),
                reports_scope: REPORTS_SCOPE.get().copied().unwrap_or(false),
                hide_added_pkgs_progress: HIDE_ADDED_PKGS_PROGRESS.get().copied().unwrap_or(false),
                is_recursive: IS_RECURSIVE.get().copied().unwrap_or(false),
                max_log_level: MAX_LOG_LEVEL.get().copied().unwrap_or(MaxLogLevel::Info),
                stream_lifecycle_output: STREAM_LIFECYCLE_OUTPUT.get().copied().unwrap_or(false),
                aggregate_output: AGGREGATE_OUTPUT.get().copied().unwrap_or(false),
                hide_lifecycle_prefix: HIDE_LIFECYCLE_PREFIX.get().copied().unwrap_or(false),
                ..state::ReporterOptions::default()
            },
        );
        let diff = diff::Diff::new(columns);
        let throttle =
            if append_only { Duration::from_secs(1) } else { Duration::from_millis(200) };
        Sink {
            state,
            diff,
            columns,
            rows,
            committed_lines: 0,
            rendered_frame_rows: 0,
            rendered_frame_outgrew_terminal: false,
            terminal_size,
            frame_buf: String::new(),
            throttle,
            last_write: None,
            prompt_active: false,
            prompt_lines: Vec::new(),
            prompt_frame: None,
        }
    }

    fn on_prompt(&mut self, action: PromptAction) {
        if is_stderr_output() {
            let mut out = std::io::stderr().lock();
            self.on_prompt_to(action, &mut out);
        } else {
            let mut out = std::io::stdout().lock();
            self.on_prompt_to(action, &mut out);
        }
    }

    fn on_prompt_to(&mut self, action: PromptAction, out: &mut impl Write) {
        match action {
            PromptAction::Start => {
                self.prompt_active = true;
                self.prompt_lines.clear();
                self.prompt_frame = None;
            }
            PromptAction::End => {
                self.prompt_active = false;
                self.diff.reset();
                self.last_write = None;
                let mut wrote = false;
                if !self.prompt_lines.is_empty() {
                    let lines = std::mem::take(&mut self.prompt_lines);
                    wrote |= self.write_output(Output::Lines(lines), out);
                }
                if let Some(frame) = self.prompt_frame.take() {
                    wrote |= self.write_output(Output::Frame(frame), out);
                }
                if wrote {
                    self.last_write = Some(Instant::now());
                }
            }
        }
    }

    fn write(&mut self, output: Output, coalesceable: bool) {
        if is_stderr_output() {
            let mut out = std::io::stderr().lock();
            self.write_to(output, coalesceable, &mut out);
        } else {
            let mut out = std::io::stdout().lock();
            self.write_to(output, coalesceable, &mut out);
        }
    }

    fn write_to(&mut self, output: Output, coalesceable: bool, out: &mut impl Write) {
        if self.prompt_active {
            match output {
                Output::None => {}
                Output::Lines(mut lines) => self.prompt_lines.append(&mut lines),
                Output::Frame(frame) => self.prompt_frame = Some(frame),
            }
            return;
        }
        // Drop a high-volume progress redraw if the throttle window hasn't
        // elapsed. State is already folded, so the next non-coalesceable
        // event (stats, summary, importing-done, the footer) renders the
        // latest counts.
        if coalesceable && self.last_write.is_some_and(|last| last.elapsed() < self.throttle) {
            return;
        }
        let wrote = self.write_output(output, out);
        if wrote {
            self.last_write = Some(Instant::now());
        }
    }

    /// Returns whether anything was written.
    fn write_output(&mut self, output: Output, out: &mut impl Write) -> bool {
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
                let lines: Vec<&str> = frame[..frame.len() - 1].split('\n').collect();
                self.refresh_terminal_size();
                // `\r` resets the column in case an external process left the
                // cursor mid-line; `\x1b[K` erases trailing characters on the
                // current line; `\x1b[0J` erases anything written below the
                // rendered frame.
                self.frame_buf.clear();
                self.frame_buf.push('\r');
                self.commit_overflow(&lines);
                let visible = &frame[frame_offset_of_line(&frame, &lines, self.committed_lines)..];
                self.diff.update_into(visible, &mut self.frame_buf);
                self.frame_buf.push_str("\x1b[K\x1b[0J");
                let _ = out.write_all(self.frame_buf.as_bytes());
            }
        }
        let _ = out.flush();
        true
    }

    /// Pick up a window resize, so the frame is fitted to the terminal it is
    /// about to be drawn on rather than the one the process started in.
    fn refresh_terminal_size(&mut self) {
        let Some((columns, rows)) = (self.terminal_size)() else { return };
        self.rows = rows;
        if columns == self.columns {
            return;
        }
        // The terminal was resized. The frame on screen has reflowed at the new
        // width, so every position the differ tracked against the old one is
        // wrong: start over below what is already there.
        self.columns = columns;
        self.diff = diff::Diff::new(columns);
    }

    /// Hands the lines of the frame that no longer fit on screen over to the
    /// scrollback, appending the differential that performs the handover to
    /// `frame_buf` and restarting the differ below them.
    ///
    /// The differ redraws by moving the cursor up from the end of its frame, so
    /// it can only reach lines that are still on screen. A frame taller than
    /// the terminal has scrolled its top away, and every later redraw then
    /// stops at the top of the screen — overwriting output above the frame
    /// instead of updating it (pnpm/pnpm#14270). Committing the overflow keeps
    /// the frame within the terminal, at the cost of no longer being able to
    /// revise what was committed.
    fn commit_overflow(&mut self, lines: &[&str]) {
        if lines.len() <= self.committed_lines {
            // The frame no longer reaches past what was committed — an error
            // frame replaces it rather than extending it. Render it whole,
            // below.
            self.committed_lines = 0;
            self.diff.reset();
            return;
        }
        let Some(rows) = self.rows else { return };
        // One row is left over for the cursor line that the trailing newline
        // puts below the frame.
        let max_rows = rows.saturating_sub(1).max(1);
        let uncommitted_rows: usize = lines[self.committed_lines..]
            .iter()
            .map(|line| rendered_rows(line, self.columns))
            .sum();
        // The last line always stays in the frame — there would be nothing left
        // to redraw otherwise — so the walk upwards starts one line above it.
        let mut first_visible = lines.len() - 1;
        let mut frame_rows = rendered_rows(lines[first_visible], self.columns);
        for idx in (self.committed_lines..first_visible).rev() {
            let line_rows = rendered_rows(lines[idx], self.columns);
            if frame_rows + line_rows > max_rows {
                break;
            }
            frame_rows += line_rows;
            first_visible = idx;
        }
        // A frame taller than the terminal has scrolled its own top away —
        // whether because a line outgrew the screen or because the window shrank
        // under it — so no cursor move reaches back into it, and growing the
        // window again does not bring it back. Start afresh below instead,
        // reprinting rather than revising, and leave the commit for the next
        // frame, whose layout is one this differ laid out itself.
        let cannot_revise =
            self.rendered_frame_outgrew_terminal || self.rendered_frame_rows > max_rows;
        if cannot_revise || first_visible == self.committed_lines {
            self.rendered_frame_rows = uncommitted_rows;
            self.rendered_frame_outgrew_terminal = uncommitted_rows > max_rows;
            if cannot_revise || self.rendered_frame_outgrew_terminal {
                self.diff = diff::Diff::new(self.columns);
            }
            return;
        }
        self.rendered_frame_rows = frame_rows;
        self.rendered_frame_outgrew_terminal = false;
        // Shrinking the frame to just the overflow leaves those lines untouched
        // where they already are, erases the rest of the frame below them, and
        // parks the cursor on the next line — where the restarted differ picks
        // up.
        let handover = format!("{}\n", lines[self.committed_lines..first_visible].join("\n"));
        let mut buf = std::mem::take(&mut self.frame_buf);
        self.diff.update_into(&handover, &mut buf);
        self.frame_buf = buf;
        self.diff = diff::Diff::new(self.columns);
        self.committed_lines = first_visible;
    }
}

/// The output terminal's `(columns, rows)`, when it has them. Measured on the
/// stream the reporter writes to, and on Windows as well as Unix — a frame is
/// fitted to the terminal it is drawn on wherever pnpm runs.
fn terminal_size() -> Option<(usize, Option<usize>)> {
    let term = if is_stderr_output() { Term::stderr() } else { Term::stdout() };
    let (rows, columns) = term.size_checked()?;
    (columns > 0).then(|| (columns as usize, (rows > 0).then_some(rows as usize)))
}

/// Where the `index`-th of `lines` starts in the `frame` they were split from.
/// The lines from there on are already laid out contiguously in `frame`, so the
/// visible part of a frame is a borrow rather than a second copy of it.
fn frame_offset_of_line(frame: &str, lines: &[&str], index: usize) -> usize {
    let trailing: usize = lines[index..].iter().map(|line| line.len() + '\n'.len_utf8()).sum();
    frame.len() - trailing
}

/// How many terminal rows `line` occupies once wrapped at `width`, counting the
/// escape sequences in it as zero-width. Never zero: an empty line still takes
/// a row. `width` is the terminal's own column count, and a zero one is read as
/// "no wrapping" rather than dividing by it.
fn rendered_rows(line: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    visible_width(line).div_ceil(width).max(1)
}

#[cfg(test)]
mod tests;
