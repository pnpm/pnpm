use clap::ValueEnum;
use pacquet_default_reporter::{DefaultReporter, SummaryScope};
use pacquet_reporter::{LogEvent, NdjsonReporter, Reporter, SilentReporter};
use std::path::Path;

/// Output format for progress and log messages.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReporterType {
    /// Rich visual output: a progress line, a packages diff, lifecycle
    /// output, and a `Done in ...` summary. The default; renders in place
    /// on a terminal and falls back to `append-only` output when stdout is
    /// not a terminal.
    Default,
    /// Like `default` but forces the append-only rendering even on a TTY —
    /// one line per update, no cursor movement.
    AppendOnly,
    /// Newline-delimited JSON on stderr.
    Ndjson,
    /// No progress output.
    Silent,
}

/// Resolve a [`ReporterType`] to the monomorphized `emit` of its sink, for
/// the event-emission sites that aren't already generic over `Reporter`.
pub(crate) fn reporter_emit(reporter: ReporterType) -> fn(&LogEvent) {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => DefaultReporter::emit,
        ReporterType::Ndjson => NdjsonReporter::emit,
        ReporterType::Silent => SilentReporter::emit,
    }
}

/// Seed the process-global default-reporter state that can't be recovered
/// from events. Idempotent; safe to call from both the fast path and the main
/// run.
pub(crate) fn configure_default_reporter(
    reporter: ReporterType,
    dir: &Path,
    summary_scope: SummaryScope,
) {
    pacquet_default_reporter::set_cwd(dir.to_string_lossy().into_owned());
    pacquet_default_reporter::set_summary_scope(summary_scope);
    if matches!(reporter, ReporterType::AppendOnly) {
        pacquet_default_reporter::force_append_only();
    }
}
