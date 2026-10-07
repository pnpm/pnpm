use clap::ValueEnum;
use pnpm_config::ColorMode;
use pnpm_default_reporter::{DefaultReporter, MaxLogLevel, SummaryScope};
use pnpm_reporter::{
    GlobalLog, LogEvent, LogLevel, NdjsonReporter, PnpmLog, Reporter, SilentReporter,
};
use std::{
    path::Path,
    sync::atomic::{AtomicU8, Ordering},
};

/// Output format for progress and log messages.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[repr(u8)]
pub enum ReporterType {
    /// Rich visual output: a progress line, a packages diff, lifecycle
    /// output, and a `Done in ...` summary. Renders in place
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

    fn resolve_with(self, config: &pnpm_config::Config) -> ReporterType {
        self.resolve(config.loglevel, config.reporter)
    }

    /// Resolve the reporter over `config` and [select](select_reporter) it.
    pub(crate) fn select_with(self, config: &pnpm_config::Config) -> ReporterType {
        let reporter = self.resolve_with(config);
        select_reporter(reporter);
        reporter
    }

    /// Resolve the reporter and seed the default reporter's log-level ceiling
    /// from the flags over `config`, for warnings emitted before the command
    /// dispatch configures the reporter.
    pub(crate) fn configure_with(self, config: &pnpm_config::Config) -> ReporterType {
        configure_max_log_level(self.loglevel.or_else(|| config.loglevel.map(Into::into)));
        self.resolve_with(config)
    }
}

/// Accepted values of pnpm's universal `--loglevel` option.
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

/// The reporter the running command drives, as a [`ReporterType`]
/// discriminant. Written only by [`select_reporter`], so the reporter a
/// handler reads and the sink [`CliReporter`] emits to cannot disagree.
static SELECTED_REPORTER: AtomicU8 = AtomicU8::new(ReporterType::Default as u8);

/// Route every later [`CliReporter`] event to `reporter`'s sink.
///
/// Call it whenever the reporter is resolved again, such as after loading
/// configuration or running the pnpmfile's `updateConfig` hook.
pub(crate) fn select_reporter(reporter: ReporterType) {
    SELECTED_REPORTER.store(reporter as u8, Ordering::Relaxed);
}

pub(crate) fn selected_reporter() -> ReporterType {
    SELECTED_REPORTER.load(Ordering::Relaxed).into()
}

/// Whether a script whose output the reporter republishes should keep its
/// colors: only a reporter that paints its own output in color shows them.
pub(crate) fn script_output_colors() -> bool {
    matches!(selected_reporter(), ReporterType::Default | ReporterType::AppendOnly)
        && pnpm_default_reporter::output_colors_enabled()
}

/// The [`Reporter`] the CLI runs commands with: forwards each event to the
/// [selected](select_reporter) sink.
///
/// Commands are instantiated with this one type rather than once per sink, so
/// the install pipeline is compiled once. Choosing the sink per event costs an
/// atomic load and a branch, next to sinks that lock a mutex or serialize JSON.
pub(crate) struct CliReporter;

impl Reporter for CliReporter {
    fn emit(event: &LogEvent) {
        if EventFilter::current().hides(event) {
            return;
        }
        reporter_emit(selected_reporter())(event);
    }

    /// While a filter is set, the error is emitted through it as a `Global`
    /// error event, so [`EventFilter::All`] stays silent.
    fn report_fatal_error(message: String) -> Option<String> {
        if EventFilter::current() != EventFilter::None {
            Self::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Error, message }));
            return None;
        }
        match selected_reporter() {
            ReporterType::Default | ReporterType::AppendOnly => {
                DefaultReporter::report_fatal_error(message)
            }
            ReporterType::Ndjson => NdjsonReporter::report_fatal_error(message),
            ReporterType::Silent => SilentReporter::report_fatal_error(message),
        }
    }
}

/// Events [`CliReporter`] drops while an [`EventFilterGuard`] is alive.
///
/// Process-global rather than task-local: the install pipeline emits from
/// rayon threads and spawned tasks. Hold a guard only around work that runs
/// alone, because it hides every [`CliReporter`] event in the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum EventFilter {
    None = 0,
    /// The lockfile-only comparison pass: hide the install-tree events and the
    /// terminal "Already up to date".
    GlobalUpdateResolution = 1,
    /// A materializing pass: `update -g` prints one summary of its own.
    GlobalUpdateMaterialization = 2,
    /// Work that runs quietly regardless of `--reporter`, such as provisioning
    /// the pnpm version a project pins.
    All = 3,
}

static EVENT_FILTER: AtomicU8 = AtomicU8::new(EventFilter::None as u8);

impl EventFilter {
    fn current() -> Self {
        match EVENT_FILTER.load(Ordering::Relaxed) {
            1 => EventFilter::GlobalUpdateResolution,
            2 => EventFilter::GlobalUpdateMaterialization,
            3 => EventFilter::All,
            _ => EventFilter::None,
        }
    }

    fn hides(self, event: &LogEvent) -> bool {
        match self {
            EventFilter::None => false,
            EventFilter::All => true,
            EventFilter::GlobalUpdateResolution => {
                matches!(
                    event,
                    LogEvent::PackageManifest(_)
                        | LogEvent::Root(_)
                        | LogEvent::Stats(_)
                        | LogEvent::Summary(_),
                ) || matches!(
                    event,
                    LogEvent::Pnpm(PnpmLog { message, .. }) if message == "Already up to date",
                )
            }
            EventFilter::GlobalUpdateMaterialization => matches!(event, LogEvent::Summary(_)),
        }
    }

    /// Apply this filter until the returned guard drops, which restores the
    /// filter that was active before, so guards can nest.
    #[must_use]
    pub(crate) fn apply(self) -> EventFilterGuard {
        EventFilterGuard { previous: EVENT_FILTER.swap(self as u8, Ordering::Relaxed) }
    }
}

pub(crate) struct EventFilterGuard {
    previous: u8,
}

impl Drop for EventFilterGuard {
    fn drop(&mut self) {
        EVENT_FILTER.store(self.previous, Ordering::Relaxed);
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

#[cfg(test)]
mod tests;
