//! The process-wide lifecycle script settings, set once by the command
//! before the first event and read into [`LifecycleOptions`].

use crate::state::LifecycleOptions;
use std::sync::OnceLock;

static STREAM_LIFECYCLE_OUTPUT: OnceLock<bool> = OnceLock::new();
static AGGREGATE_OUTPUT: OnceLock<bool> = OnceLock::new();
static HIDE_LIFECYCLE_PREFIX: OnceLock<bool> = OnceLock::new();
static SUMMARIZE_SCRIPTS: OnceLock<bool> = OnceLock::new();

/// Stream lifecycle script output line by line instead of collecting it
/// into a collapsible block, pnpm's `--stream`. Call once before the
/// first event.
pub fn stream_lifecycle_output() {
    let _ = STREAM_LIFECYCLE_OUTPUT.set(true);
}

/// Hold each script's streamed output until it exits, then print the run
/// as one block, pnpm's `--aggregate-output`. Call once before the first
/// event.
pub fn aggregate_output() {
    let _ = AGGREGATE_OUTPUT.set(true);
}

/// Drop the project prefix from streamed script output lines, pnpm's
/// `--reporter-hide-prefix`. Call once before the first event.
pub fn hide_lifecycle_prefix() {
    let _ = HIDE_LIFECYCLE_PREFIX.set(true);
}

/// Summarize the lifecycle scripts in the checks footer, see
/// [`LifecycleOptions::summarize_scripts`].
/// Call once before the first event.
pub fn summarize_scripts() {
    let _ = SUMMARIZE_SCRIPTS.set(true);
}

pub(crate) fn lifecycle_options() -> LifecycleOptions {
    LifecycleOptions {
        stream_output: STREAM_LIFECYCLE_OUTPUT.get().is_some_and(|value| *value),
        aggregate_output: AGGREGATE_OUTPUT.get().is_some_and(|value| *value),
        hide_prefix: HIDE_LIFECYCLE_PREFIX.get().is_some_and(|value| *value),
        summarize_scripts: SUMMARIZE_SCRIPTS.get().is_some_and(|value| *value),
        ..Default::default()
    }
}
