use clap::ValueEnum;
use pnpm_default_reporter::{DefaultReporter, MaxLogLevel, SummaryScope};
use pnpm_reporter::{LogEvent, NdjsonReporter, Reporter, SilentReporter};
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

/// Accepted values of pnpm's universal `--loglevel` option.
///
/// `silent` selects the silent reporter outright (see
/// [`super::cli_command::CliArgs::effective_reporter`]); the other values
/// become the default reporter's [`MaxLogLevel`] ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogLevelSetting {
    Silent,
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevelSetting {
    fn as_max_log_level(self) -> Option<MaxLogLevel> {
        match self {
            LogLevelSetting::Silent => None,
            LogLevelSetting::Error => Some(MaxLogLevel::Error),
            LogLevelSetting::Warn => Some(MaxLogLevel::Warn),
            LogLevelSetting::Info => Some(MaxLogLevel::Info),
            LogLevelSetting::Debug => Some(MaxLogLevel::Debug),
        }
    }
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
/// from events. Idempotent — the first call wins, so it has to run before
/// anything can emit.
pub(crate) fn configure_default_reporter(
    reporter: ReporterType,
    dir: &Path,
    summary_scope: SummaryScope,
    reports_scope: bool,
    hide_added_pkgs_progress: bool,
    is_recursive: bool,
    use_stderr: bool,
) {
    pnpm_default_reporter::set_cwd(dir.to_string_lossy().into_owned());
    if use_stderr {
        pnpm_default_reporter::use_stderr();
    }
    pnpm_default_reporter::set_summary_scope(summary_scope);
    pnpm_default_reporter::set_reports_scope(reports_scope);
    pnpm_default_reporter::set_hide_added_pkgs_progress(hide_added_pkgs_progress);
    pnpm_default_reporter::set_is_recursive(is_recursive);
    if matches!(reporter, ReporterType::AppendOnly) {
        pnpm_default_reporter::force_append_only();
    }
}

/// Seed the default reporter's verbosity ceiling from the `--loglevel`
/// value. `silent` and an absent flag leave the [`MaxLogLevel::Info`]
/// default in place — `silent` never reaches the default reporter (see
/// [`super::cli_command::CliArgs::effective_reporter`]).
pub(crate) fn configure_max_log_level(loglevel: Option<LogLevelSetting>) {
    if let Some(level) = loglevel.and_then(LogLevelSetting::as_max_log_level) {
        pnpm_default_reporter::set_max_log_level(level);
    }
}
