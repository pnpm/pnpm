//! Bridge from pacquet's static-dispatch [`Reporter`] to a JS callback.
//!
//! `pacquet_reporter::Reporter` is a compile-time trait with an associated
//! `emit(event)` and no `&self`, so an implementation cannot capture a
//! per-call closure — any state must live in a `static`. [`NodeBridgeReporter`]
//! therefore forwards each event, serialized to JSON, into a process-global
//! [`ThreadsafeFunction`] that a JS host installs for the duration of one
//! engine call. pacquet's `LogEvent` stream is wire-compatible with
//! `@pnpm/core-loggers`, so the JS side can feed the events straight into
//! `@pnpm/logger`'s `streamParser` and render with `@pnpm/cli.default-reporter`.
//!
//! Only one engine operation runs against the sink at a time (installs are
//! serialized per directory on the JS side), so a single global slot is
//! sufficient. `emit` never blocks and never panics: a missing sink or a full
//! queue drops the event, matching the "a reporter problem can never crash an
//! install" contract on the trait.

use std::sync::{Mutex, OnceLock, RwLock};

use napi::{
    Status,
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode, UnknownReturnValue},
};
use pacquet_reporter::{LogEvent, Reporter, StatsMessage};

/// A JS `(event: object) => void` callback. `CalleeHandled = false` so the JS
/// side is invoked with just the event (no leading error argument); the return
/// value is discarded ([`UnknownReturnValue`]). Non-blocking; errors in the JS
/// callback are its own concern and never propagate back into the engine.
pub type LogSink =
    ThreadsafeFunction<serde_json::Value, UnknownReturnValue, serde_json::Value, Status, false>;

/// Process-global sink. `RwLock<Option<..>>` rather than a bare `OnceLock`
/// because the sink is installed and cleared around each engine call.
fn sink_slot() -> &'static RwLock<Option<LogSink>> {
    static SLOT: OnceLock<RwLock<Option<LogSink>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

/// Install `sink` as the global log sink for the duration of an engine call.
/// A previously installed sink is replaced and returned to the caller so it
/// can be restored (engine calls do not currently nest, but this keeps the
/// contract explicit).
pub fn set_global_log_sink(sink: LogSink) -> Option<LogSink> {
    match sink_slot().write() {
        Ok(mut guard) => guard.replace(sink),
        Err(_) => None,
    }
}

/// Clear the global log sink after an engine call completes.
pub fn clear_global_log_sink() {
    if let Ok(mut guard) = sink_slot().write() {
        *guard = None;
    }
}

/// Install outcome accumulated from the reporter event stream, since
/// `pacquet_package_manager::Install::run` itself returns `()`. The
/// `pnpm:stats` channel carries added/removed counts and
/// `pnpm:ignored-scripts` the packages whose build scripts were blocked
/// (pnpm's `depsRequiringBuild`).
#[derive(Default)]
pub struct InstallStats {
    pub added: u64,
    pub removed: u64,
    pub deps_requiring_build: Vec<String>,
}

/// Process-global stats accumulator, active only while an install runs. A
/// global (rather than per-call) accumulator is safe because engine calls that
/// collect stats are serialized behind [`crate::install::install_lock`].
fn stats_slot() -> &'static Mutex<Option<InstallStats>> {
    static SLOT: OnceLock<Mutex<Option<InstallStats>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Begin accumulating install stats. Any previously accumulated stats are
/// discarded.
pub fn begin_stats() {
    if let Ok(mut guard) = stats_slot().lock() {
        *guard = Some(InstallStats::default());
    }
}

/// Stop accumulating and return the collected stats (default when none were
/// accumulated).
pub fn take_stats() -> InstallStats {
    stats_slot().lock().ok().and_then(|mut guard| guard.take()).unwrap_or_default()
}

fn accumulate_stats(event: &LogEvent) {
    let Ok(mut guard) = stats_slot().lock() else { return };
    let Some(stats) = guard.as_mut() else { return };
    match event {
        LogEvent::Stats(log) => match &log.message {
            StatsMessage::Added { added, .. } => stats.added += *added,
            StatsMessage::Removed { removed, .. } => stats.removed += *removed,
        },
        LogEvent::IgnoredScripts(log) => {
            for name in &log.package_names {
                if !stats.deps_requiring_build.contains(name) {
                    stats.deps_requiring_build.push(name.clone());
                }
            }
        }
        _ => {}
    }
}

/// [`Reporter`] that forwards every event to the global JS sink.
pub struct NodeBridgeReporter;

impl Reporter for NodeBridgeReporter {
    fn emit(event: &LogEvent) {
        accumulate_stats(event);
        // Serialize outside the lock; drop the event on any failure.
        let Ok(value) = serde_json::to_value(event) else { return };
        let Ok(guard) = sink_slot().read() else { return };
        if let Some(sink) = guard.as_ref() {
            // Non-blocking enqueue. A closed or saturated queue drops the
            // event rather than blocking a rayon/tokio worker.
            sink.call(value, ThreadsafeFunctionCallMode::NonBlocking);
        }
    }
}
