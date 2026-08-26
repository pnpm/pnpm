//! Bridge from pacquet's static-dispatch [`Reporter`] to a JS callback.
//!
//! `pnpm_reporter::Reporter` is a compile-time trait with an associated
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
use pnpm_reporter::{LogEvent, Reporter, StatsMessage};

use crate::native_reporter::NativeRenderer;

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

/// Process-global renderer, installed only while an engine call asked for
/// pnpm's own terminal output. A `Mutex` rather than an `RwLock` because
/// folding an event mutates the renderer; engine calls are serialized
/// behind [`crate::install::engine_call_lock`], so the only contention is
/// between the worker threads of one call, which is what the reporter
/// state needs serialized anyway.
fn renderer_slot() -> &'static Mutex<Option<NativeRenderer>> {
    static SLOT: OnceLock<Mutex<Option<NativeRenderer>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Install `renderer` as the global renderer for one engine call,
/// returning the one it replaced.
pub fn set_global_renderer(renderer: NativeRenderer) -> Option<NativeRenderer> {
    match renderer_slot().lock() {
        Ok(mut guard) => guard.replace(renderer),
        Err(_) => None,
    }
}

/// Clear the global renderer after an engine call completes.
pub fn clear_global_renderer() {
    if let Ok(mut guard) = renderer_slot().lock() {
        *guard = None;
    }
}

/// Install outcome accumulated from the reporter event stream, since
/// `pnpm_package_manager::Install::run` itself returns `()`. The
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
/// collect stats are serialized behind [`crate::install::engine_call_lock`].
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

/// RAII guard over one engine call's global reporter state. Installs `sink`
/// (when the caller supplied one) as the process-global log sink and restores
/// the previously installed sink on drop — including on unwind. Without this, a
/// panic in a worker thread would leave a stale sink installed, misrouting a
/// later caller's log events to a dead JS callback. Any un-taken stats are also
/// discarded on drop so a panicked call cannot bleed counts into the next one.
pub struct EngineCallGuard {
    prev_sink: Option<LogSink>,
    installed: bool,
    prev_renderer: Option<NativeRenderer>,
    renderer_installed: bool,
}

impl EngineCallGuard {
    pub fn new(sink: Option<LogSink>) -> Self {
        Self::with_renderer(sink, None)
    }

    pub fn with_renderer(sink: Option<LogSink>, renderer: Option<NativeRenderer>) -> Self {
        let (prev_sink, installed) = match sink {
            Some(sink) => (set_global_log_sink(sink), true),
            None => (None, false),
        };
        let (prev_renderer, renderer_installed) = match renderer {
            Some(renderer) => (set_global_renderer(renderer), true),
            None => (None, false),
        };
        Self { prev_sink, installed, prev_renderer, renderer_installed }
    }
}

impl Drop for EngineCallGuard {
    fn drop(&mut self) {
        if self.installed {
            match self.prev_sink.take() {
                Some(prev) => {
                    set_global_log_sink(prev);
                }
                None => clear_global_log_sink(),
            }
        }
        if self.renderer_installed {
            match self.prev_renderer.take() {
                Some(prev) => {
                    set_global_renderer(prev);
                }
                None => clear_global_renderer(),
            }
        }
        let _ = take_stats();
    }
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
        render_natively(event);
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

/// Fold the event into the installed renderer, if the call asked for
/// pnpm's own output. A poisoned lock (a panic while rendering) silently
/// stops the output rather than propagating: the reporter contract is that
/// a reporter problem can never fail an install.
fn render_natively(event: &LogEvent) {
    let Ok(mut guard) = renderer_slot().lock() else { return };
    if let Some(renderer) = guard.as_mut() {
        renderer.handle(event);
    }
}

#[cfg(test)]
mod tests;
