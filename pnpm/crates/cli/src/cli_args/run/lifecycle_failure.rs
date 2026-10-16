use super::{RunContext, ScriptExit, ScriptOutput};
use crate::cli_args::reporter::default_reporter_selected;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};

/// Report the `[ELIFECYCLE]` line for a `stage` that failed with `status`.
///
/// A streamed run goes through the reporter, which may be redrawing its
/// frame in place: a line written past it would shift the rows the frame
/// moves the cursor over. An inherited run under the default reporter
/// writes the line to stderr after the script's own output; under any
/// other reporter it goes through the reporter too, so the ndjson
/// reporter writes a record and the silent reporter drops it.
pub(super) fn report_lifecycle_failure(ctx: &RunContext<'_>, status: ScriptExit, stage: &str) {
    let message = lifecycle_failure_message(status, stage);
    if matches!(ctx.output, ScriptOutput::Inherit) && default_reporter_selected() {
        pnpm_diagnostics::eprintln!("{message}");
        return;
    }
    (ctx.emit)(&LogEvent::Pnpm(PnpmLog { level: LogLevel::Error, message, prefix: String::new() }));
}

fn lifecycle_failure_message(status: ScriptExit, stage: &str) -> String {
    if let Some(signal) = status.signal_name() {
        format!("[ELIFECYCLE] Command failed with signal {signal}.")
    } else if stage == "test" {
        "[ELIFECYCLE] Test failed. See above for more details.".to_string()
    } else if let Some(code) = status.code() {
        format!("[ELIFECYCLE] Command failed with exit code {code}.")
    } else {
        "[ELIFECYCLE] Command failed.".to_string()
    }
}
