use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_reporter::LifecycleStdio;
use std::io;

/// Failure to run a script under the `shellEmulator` setting. A script
/// that runs and exits non-zero is not an error here — the exit code is
/// returned to the caller, which decides what a failure means.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ShellEmulatorError {
    #[display("Failed to parse `{script}` with the shell emulator: {message}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SHELL_EMULATOR_PARSE))]
    Parse { script: String, message: String },

    #[display("Failed to start the shell emulator for `{script}`: {source}")]
    #[diagnostic(code(ERR_PNPM_EXECUTOR_SHELL_EMULATOR_START))]
    Start {
        script: String,
        #[error(source)]
        source: io::Error,
    },
}

/// Where an emulated script's output goes.
#[derive(Clone, Copy)]
pub enum EmulatedOutput<'a> {
    /// Straight to pacquet's own stdout and stderr, for a foreground
    /// `pnpm run`.
    Inherit,
    /// One call per output line, tagged with the stream it came from,
    /// for the install-time path that turns lines into reporter events.
    Lines(&'a (dyn Fn(LifecycleStdio, String) + Sync)),
}
