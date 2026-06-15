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
pub mod format;
pub mod state;

use std::{
    fmt::Write as _,
    io::{IsTerminal, Write},
    sync::{LazyLock, Mutex, OnceLock},
};

use pacquet_reporter::{LogEvent, Reporter};

use crate::{
    colors::Colors,
    state::{Output, ReporterState},
};

static CWD: OnceLock<String> = OnceLock::new();
static PACKAGE_VERSION: OnceLock<String> = OnceLock::new();

/// Set the project root the reporter renders paths relative to. Call once
/// before the first event; ignored if already set.
pub fn set_cwd(cwd: impl Into<String>) {
    let _ = CWD.set(cwd.into());
}

/// Set the version rendered in the `Done in ... using pacquet v<version>`
/// footer. Call once before the first event; ignored if already set.
pub fn set_package_version(version: impl Into<String>) {
    let _ = PACKAGE_VERSION.set(version.into());
}

pub(crate) fn package_version() -> &'static str {
    PACKAGE_VERSION.get().map(String::as_str).unwrap_or(env!("CARGO_PKG_VERSION"))
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
        sink.write(output);
    }
}

static SINK: LazyLock<Mutex<Sink>> = LazyLock::new(|| Mutex::new(Sink::new()));

struct Sink {
    state: ReporterState,
    columns: usize,
    prev_rows: usize,
}

impl Sink {
    fn new() -> Self {
        let is_tty = std::io::stdout().is_terminal();
        let columns = if is_tty { terminal_columns().unwrap_or(80) } else { 80 };
        // pnpm's `outputMaxWidth`: `columns - 2` on a TTY, else 80.
        let width = if is_tty { columns.saturating_sub(2) } else { 80 };
        let colors = Colors { enabled: is_tty && std::env::var_os("NO_COLOR").is_none() };
        let state = ReporterState::new(cwd(), width, colors, !is_tty);
        Sink { state, columns, prev_rows: 0 }
    }

    fn write(&mut self, output: Output) {
        let mut out = std::io::stdout().lock();
        match output {
            Output::None => return,
            Output::Lines(lines) => {
                for line in lines {
                    let _ = writeln!(out, "{line}");
                }
            }
            Output::Frame(frame) => {
                let mut buf = String::new();
                if self.prev_rows > 0 {
                    let _ = write!(buf, "\x1b[{}A", self.prev_rows);
                }
                buf.push_str("\x1b[0J");
                buf.push_str(&frame);
                buf.push('\n');
                let _ = out.write_all(buf.as_bytes());
                self.prev_rows = count_rows(&frame, self.columns);
            }
        }
        let _ = out.flush();
    }
}

/// Terminal rows a frame occupies, accounting for soft-wrapping at
/// `columns`. Used to know how far to move the cursor up before redrawing.
fn count_rows(frame: &str, columns: usize) -> usize {
    if columns == 0 {
        return frame.split('\n').count();
    }
    frame
        .split('\n')
        .map(|line| {
            let width = format::visible_width(line);
            if width == 0 { 1 } else { width.div_ceil(columns) }
        })
        .sum()
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
