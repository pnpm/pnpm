//! Emit `pnpm:global` info / warn messages through the `R: Reporter` seam,
//! matching pnpm's `globalInfo` / `globalWarn`.

use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};

pub(crate) fn global_info<R: Reporter>(message: &str) {
    R::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Info, message: message.to_owned() }));
}

pub(crate) fn global_warn<R: Reporter>(message: &str) {
    R::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message: message.to_owned() }));
}
