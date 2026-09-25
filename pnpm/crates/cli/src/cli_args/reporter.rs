use clap::ValueEnum;
use pnpm_config::ColorMode;
use pnpm_default_reporter::{DefaultReporter, MaxLogLevel, SummaryScope};
use pnpm_reporter::{LogEvent, NdjsonReporter, Reporter, SilentReporter};
use std::path::Path;

/// Output format for progress and log messages.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[repr(u8)]
pub enum ReporterType {
    /// Rich visual output: a progress line, a packages diff, lifecycle
    /// output, and a `Done in ...` summary. The default; renders in place
    /// on a terminal and falls back to `append-only` output when stdout is
    /// not a terminal.
    #[default]
    Default = 0,
    /// Like `default` but forces the append-only rendering even on a TTY —
    /// one line per update, no cursor movement.
    AppendOnly = 1,
    /// Newline-delimited JSON on stderr.
    Ndjson = 2,
    /// No progress output.
    Silent = 3,
}

impl From<u8> for ReporterType {
    fn from(value: u8) -> Self {
        match value {
            0 => ReporterType::Default,
            1 => ReporterType::AppendOnly,
            2 => ReporterType::Ndjson,
            3 => ReporterType::Silent,
            _ => unreachable!("invalid reporter discriminant: {value}"),
        }
    }
}

impl From<pnpm_config::ReporterType> for ReporterType {
    fn from(reporter: pnpm_config::ReporterType) -> Self {
        match reporter {
            pnpm_config::ReporterType::Default => ReporterType::Default,
            pnpm_config::ReporterType::AppendOnly => ReporterType::AppendOnly,
            pnpm_config::ReporterType::Ndjson => ReporterType::Ndjson,
            pnpm_config::ReporterType::Silent => ReporterType::Silent,
        }
    }
}

/// The `--reporter` and `--loglevel` flags as given on the command line,
/// before the configuration they take precedence over is loaded.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ReporterFlags {
    pub(crate) reporter: Option<ReporterType>,
    pub(crate) loglevel: Option<LogLevelSetting>,
}

impl ReporterFlags {
    /// The reporter the command should drive: `--loglevel silent` or configured
    /// `loglevel: silent` forces the silent reporter over any `--reporter` choice.
    /// Otherwise `--reporter` wins over the configured `reporter` setting,
    /// mirroring the reporter selection in pnpm 11's `main.ts`.
    pub(crate) fn resolve(
        self,
        config_loglevel: Option<pnpm_config::LogLevel>,
        config_reporter: Option<pnpm_config::ReporterType>,
    ) -> ReporterType {
        if self.loglevel.or_else(|| config_loglevel.map(Into::into))
            == Some(LogLevelSetting::Silent)
        {
            return ReporterType::Silent;
        }
        self.reporter
            .or_else(|| config_reporter.map(Into::into))
            .unwrap_or_default()
    }

    pub(crate) fn resolve_with(self, config: &pnpm_config::Config) -> ReporterType {
        self.resolve(config.loglevel, config.reporter)
    }

    /// Resolve the reporter and seed the default reporter's log-level ceiling
    /// from the flags over `config`, for warnings emitted before the command
    /// dispatch configures the reporter.
    pub(crate) fn configure_with(self, config: &pnpm_config::Config) -> fn(&LogEvent) {
        configure_max_log_level(self.loglevel.or_else(|| config.loglevel.map(Into::into)));
        reporter_emit(self.resolve_with(config))
    }
}

/// Accepted values of pnpm's universal `--loglevel` option.
///
/// `silent` selects the silent reporter outright (see
/// [`ReporterFlags::resolve`]); the other values
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

impl From<pnpm_config::LogLevel> for LogLevelSetting {
    fn from(level: pnpm_config::LogLevel) -> Self {
        match level {
            pnpm_config::LogLevel::Silent => LogLevelSetting::Silent,
            pnpm_config::LogLevel::Error => LogLevelSetting::Error,
            pnpm_config::LogLevel::Warn => LogLevelSetting::Warn,
            pnpm_config::LogLevel::Info => LogLevelSetting::Info,
            pnpm_config::LogLevel::Debug => LogLevelSetting::Debug,
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

/// The process-global default-reporter state that can't be recovered from
/// events, as [`configure_default_reporter`] takes it.
///
/// The `bool` fields are all opt-in: a `false` leaves whatever an earlier
/// call configured, so a value the command line did not carry can still
/// arrive from the loaded configuration.
pub(crate) struct DefaultReporterSetup<'a> {
    pub(crate) reporter: ReporterType,
    pub(crate) dir: &'a Path,
    pub(crate) summary_scope: SummaryScope,
    pub(crate) reports_scope: bool,
    pub(crate) hide_added_pkgs_progress: bool,
    pub(crate) is_recursive: bool,
    pub(crate) lifecycle: LifecycleReporterSetup,
}

pub(crate) struct LifecycleReporterSetup {
    pub(crate) use_stderr: bool,
    pub(crate) stream_output: bool,
    pub(crate) aggregate_output: bool,
    pub(crate) hide_prefix: bool,
}

/// Seed the process-global default-reporter state that can't be recovered
/// from events. Idempotent — the first call wins, so it has to run before
/// anything can emit.
pub(crate) fn configure_default_reporter(setup: &DefaultReporterSetup<'_>) {
    pnpm_default_reporter::set_cwd(setup.dir.to_string_lossy().into_owned());
    if setup.lifecycle.use_stderr {
        pnpm_default_reporter::use_stderr();
    }
    if setup.lifecycle.stream_output {
        pnpm_default_reporter::stream_lifecycle_output();
    }
    if setup.lifecycle.aggregate_output {
        pnpm_default_reporter::aggregate_output();
    }
    if setup.lifecycle.hide_prefix {
        pnpm_default_reporter::hide_lifecycle_prefix();
    }
    pnpm_default_reporter::set_summary_scope(setup.summary_scope);
    pnpm_default_reporter::set_reports_scope(setup.reports_scope);
    pnpm_default_reporter::set_hide_added_pkgs_progress(setup.hide_added_pkgs_progress);
    pnpm_default_reporter::set_is_recursive(setup.is_recursive);
    if matches!(setup.reporter, ReporterType::AppendOnly) {
        pnpm_default_reporter::force_append_only();
    }
}

/// Seed the default reporter's verbosity ceiling from the `--loglevel`
/// value. `silent` and an absent flag leave the [`MaxLogLevel::Info`]
/// default in place — `silent` never reaches the default reporter (see
/// [`ReporterFlags::resolve`]).
pub(crate) fn configure_max_log_level(loglevel: Option<LogLevelSetting>) {
    if let Some(level) = loglevel.and_then(LogLevelSetting::as_max_log_level) {
        pnpm_default_reporter::set_max_log_level(level);
    }
}

/// Whether info-level output written outside the reporter, such as the
/// `$ <script>` echo before a foreground script, is suppressed: under the
/// silent reporter, or when `--loglevel` / `loglevel` is `warn` or `error`.
pub(crate) fn suppresses_info_output(reporter: ReporterType) -> bool {
    matches!(reporter, ReporterType::Silent)
        || pnpm_default_reporter::max_log_level() < MaxLogLevel::Info
}

/// The `--loglevel` flag a spawned pnpm needs to keep the parent's
/// `warn` / `error` ceiling; `None` at the default `info` and above.
pub(crate) fn quiet_loglevel_arg() -> Option<&'static str> {
    match pnpm_default_reporter::max_log_level() {
        MaxLogLevel::Error => Some("--loglevel=error"),
        MaxLogLevel::Warn => Some("--loglevel=warn"),
        MaxLogLevel::Info | MaxLogLevel::Debug => None,
    }
}

pub(crate) fn configure_color(mode: ColorMode) {
    pnpm_default_reporter::set_color_mode(mode);
    match mode {
        ColorMode::Always => owo_colors::set_override(true),
        ColorMode::Auto => owo_colors::unset_override(),
        ColorMode::Never => owo_colors::set_override(false),
    }
}
