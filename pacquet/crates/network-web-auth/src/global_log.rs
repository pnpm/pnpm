//! The web-auth flow's equivalent of pnpm's `globalInfo` / `globalWarn`.
//!
//! pnpm writes these messages to a `bole('pnpm:global')` logger; pacquet
//! routes them through the [`Reporter`] seam on the matching
//! [`LogEvent::Global`] channel, so the `--reporter` choice (silent vs
//! ndjson) still applies. Threaded as `R: Reporter` rather than a
//! capability on `Sys` because the sink is a runtime choice, not a system
//! facility.

use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};

pub(crate) fn global_info<R: Reporter>(message: &str) {
    R::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Info, message: message.to_owned() }));
}

pub(crate) fn global_warn<R: Reporter>(message: &str) {
    R::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message: message.to_owned() }));
}
