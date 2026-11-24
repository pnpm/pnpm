//! The gate the `progress` setting puts in front of the progress streams.

use std::sync::{
    OnceLock,
    atomic::{
        AtomicBool,
        Ordering,
    },
};

use pnpm_reporter::{
    FetchingProgressMessage,
    LogEvent,
};

static ENABLED: AtomicBool = AtomicBool::new(true);
static HIDE_PREFIX: OnceLock<bool> = OnceLock::new();

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

/// Drop the project prefix from progress lines, see
/// [`ProgressOptions::hide_prefix`](crate::state::ProgressOptions::hide_prefix).
/// Call once before the first event.
pub fn hide_progress_prefix() {
    let _ = HIDE_PREFIX.set(true);
}

pub(crate) fn prefix_hidden() -> bool {
    HIDE_PREFIX.get().is_some_and(|value| *value)
}

/// Whether `event` is one of the progress updates a disabled `progress`
/// setting drops. Only the two progress streams are gated: warnings,
/// lifecycle output, and the summary still render.
pub(crate) fn is_suppressed(event: &LogEvent) -> bool {
    !ENABLED.load(Ordering::Relaxed)
        && matches!(event, LogEvent::Progress(_) | LogEvent::FetchingProgress(_))
}

/// Whether an event is a high-volume progress update that may be dropped
/// under throttling, mirroring pnpm's `throttleProgress` on the progress
/// stream.
pub(crate) fn is_coalesceable(event: &LogEvent) -> bool {
    match event {
        LogEvent::Progress(_) => true,
        LogEvent::FetchingProgress(log) => {
            matches!(log.message, FetchingProgressMessage::InProgress { .. })
        }
        _ => false,
    }
}
