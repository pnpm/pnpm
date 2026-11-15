use super::{Deserialize, LogEntryFile};

/// Runtime logging configuration. Mirrors the YAML `log:` object
/// (Verdaccio 6+ shape). Drives the `tracing-subscriber` init in the
/// binary: format selects human-readable vs NDJSON, level seeds the
/// default `EnvFilter`. Records go to stdout.
#[derive(Debug, Default, Clone)]
pub struct LogConfig {
    pub format: LogFormat,
    pub level: LogLevel,
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
    /// tracing level and filters the same as `info`: the per-request
    /// `pnpr::access` records are emitted at `info`, so both levels
    /// include them. Structured audit records (`pnpr::audit=info`) are
    /// retained across all levels so security events are not lost.
    #[must_use]
    pub fn as_filter_directive(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Http => "info,pnpr::access=info,pnpr::audit=info",
            LogLevel::Info => "info,pnpr::audit=info",
            LogLevel::Warn => "warn,pnpr::audit=info",
            LogLevel::Error => "error,pnpr::audit=info",
        }
    }
}

/// Lift the YAML `log:` object's `format` / `level` onto runtime
/// defaults. Missing block = default pretty/info config; missing
/// individual fields fall back to their `Default` impls.
pub(super) fn build_log_config(entry: Option<&LogEntryFile>) -> LogConfig {
    let Some(entry) = entry else { return LogConfig::default() };
    LogConfig { format: entry.format.unwrap_or_default(), level: entry.level.unwrap_or_default() }
}
