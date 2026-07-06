use clap::ValueEnum;
use pacquet_default_reporter::DefaultReporter;
use pacquet_reporter::{LogEvent, NdjsonReporter, Reporter, SilentReporter};
use std::path::Path;

/// Selectable rendering strategy for log events.
///
/// Mirrors the names pnpm uses for `--reporter` (`default`, `append-only`,
/// `ndjson`, `silent`).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReporterType {
    /// pnpm-style visual output (progress line, packages diff, lifecycle
    /// output, `Done in ...`). The default; renders in place on a TTY and
    /// falls back to append-only when stdout is not a TTY.
    Default,
    /// Like `default` but forces the append-only rendering even on a TTY —
    /// one line per update, no cursor movement.
    AppendOnly,
    /// Newline-delimited JSON in pnpm's wire format on stderr.
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
    filter_summary_by_prefix: bool,
) {
    pacquet_default_reporter::set_cwd(dir.to_string_lossy().into_owned());
    pacquet_default_reporter::set_filter_summary_by_prefix(filter_summary_by_prefix);
    if matches!(reporter, ReporterType::AppendOnly) {
        pacquet_default_reporter::force_append_only();
    }
}
