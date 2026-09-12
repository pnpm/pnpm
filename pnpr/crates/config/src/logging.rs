use super::{Deserialize, LogEntryFile};

/// Runtime logging configuration. Mirrors the YAML `log:` object
/// (Verdaccio 6+ shape). Drives the `tracing-subscriber` init in the
/// binary: format selects human-readable vs NDJSON, level seeds the
/// default `EnvFilter`.
///
/// Only `type: stdout` is honored — file and syslog sinks are future
/// work. An unsupported `type:` still parses (so a verdaccio config
/// can be copied in untouched) but is ignored at runtime, with a
/// warning logged once the subscriber is up.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub format: LogFormat,
    pub level: LogLevel,
    /// The configured sink (`log.type`). Only [`Self::STDOUT_SINK`]
    /// is implemented; any other value is recorded here so the binary
    /// can warn about it at startup.
    pub sink: String,
}

impl LogConfig {
    /// The single sink pnpr actually writes to.
    pub const STDOUT_SINK: &'static str = "stdout";

    /// Whether the configured sink is one the server implements.
    #[must_use]
    pub fn sink_is_supported(&self) -> bool {
        self.sink == Self::STDOUT_SINK
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            level: LogLevel::default(),
            sink: Self::STDOUT_SINK.to_string(),
        }
    }
}

/// Wire format for log records. `Pretty` is human-readable with
/// colors when stdout is a TTY; `Json` is NDJSON (one JSON object
/// per record) suitable for log shippers — the same shape pino
/// emits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

/// Severity threshold. Maps onto `tracing::Level` plus a synthetic
/// `Http` mid-tier (between info and debug) that mirrors what
/// verdaccio and pino call "the request-log level."
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Http,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Convert to an `EnvFilter` directive string. `http` is not a
    /// tracing level, so we expand it to `info` for the framework
    /// plus a `pnpr::access=info` target so the per-request
    /// access log surfaces even when the rest of the crate is
    /// quieter.
    #[must_use]
    pub fn as_filter_directive(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Http => "info,pnpr::access=info",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// Lift the YAML `log:` object's `format` / `level` onto runtime
/// defaults. Missing block = default pretty/info config; missing
/// individual fields fall back to their `Default` impls.
pub(super) fn build_log_config(entry: Option<&LogEntryFile>) -> LogConfig {
    let Some(entry) = entry else { return LogConfig::default() };
    LogConfig {
        format: entry.format.unwrap_or_default(),
        level: entry.level.unwrap_or_default(),
        sink: entry.r#type.clone(),
    }
}
