//! Emit `pnpm:global` info / warn messages through the `R: Reporter` seam.

use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};

pub(crate) fn global_info<Reporter: self::Reporter>(message: String) {
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Info, message }));
}

pub(crate) fn global_warn<Reporter: self::Reporter>(message: String) {
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}
