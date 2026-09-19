//! The gate the `progress` setting puts in front of the progress streams.

use std::sync::atomic::{AtomicBool, Ordering};

use pnpm_reporter::LogEvent;

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Configure whether dependency and download progress is rendered. Starts
/// enabled, as the `progress` setting defaults to.
///
/// Unlike the settings the reporter keeps once it is configured, this one
/// stays writable: the command line seeds it before the first event can be
/// emitted, and the `progress` setting overwrites it once the configuration
/// has been loaded.
pub fn set_progress(progress: bool) {
    ENABLED.store(progress, Ordering::Relaxed);
}

/// Whether `event` is one of the progress updates a disabled `progress`
/// setting drops. Only the two progress streams are gated: warnings,
/// lifecycle output, and the summary still render.
pub(crate) fn is_suppressed(event: &LogEvent) -> bool {
    !ENABLED.load(Ordering::Relaxed)
        && matches!(event, LogEvent::Progress(_) | LogEvent::FetchingProgress(_))
}
