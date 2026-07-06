//! Emit `pnpm:global` info / warn messages through the `R: Reporter` seam.

use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};

pub(crate) fn global_info<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: message.to_owned(),
    }));
}

pub(crate) fn global_warn<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: message.to_owned(),
    }));
}
