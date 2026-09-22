//! Going on with an interpreter the project's `requires-python` rejects.
//!
//! `requires-python` is to an interpreter what `engines.runtime` is to a
//! Node.js runtime, so `runtimeOnFail` decides this the same way.

use super::Interpreter;
use miette::{
    Result,
    bail,
};
use pnpm_config::{
    Config,
    RuntimeOnFail,
};
use pnpm_reporter::{
    GlobalLog,
    LogEvent,
    LogLevel,
    Reporter,
};
use std::path::Path;

/// What an install does with an interpreter outside the project's
/// `requires-python`, as `runtimeOnFail` says. `download` and the unset
/// default install one instead, and reach this only once the release
/// offers none.
#[derive(Clone, Copy)]
pub(crate) enum Mismatch {
    Fail,
    Warn,
    Ignore,
}

impl Mismatch {
    pub(crate) fn of(config: &Config) -> Self {
        match config.runtime_on_fail {
            Some(RuntimeOnFail::Warn) => Self::Warn,
            Some(RuntimeOnFail::Ignore) => Self::Ignore,
            None | Some(RuntimeOnFail::Download | RuntimeOnFail::Error) => Self::Fail,
        }
    }

    /// Whether the install goes on with an interpreter the range
    /// rejects.
    pub(crate) fn bypassed(self) -> bool {
        !matches!(self, Self::Fail)
    }
}

/// Whether the install goes on with `version`, which the project's range
/// rejects. `warn` says which interpreter it went on with, and `ignore`
/// says nothing.
pub(super) fn accepted<Reporter: self::Reporter + 'static>(
    root: &Path,
    version: &pep440_rs::Version,
    specifiers: &pep440_rs::VersionSpecifiers,
    mismatch: Mismatch,
) -> bool {
    if matches!(mismatch, Mismatch::Warn) {
        Reporter::emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Warn,
            message: format!(
                "Installing {} with Python {version}, which the project's requires-python {specifiers} rejects",
                root.display(),
            ),
        }));
    }
    mismatch.bypassed()
}

/// A project that pins an interpreter range cannot be installed with the
/// interpreter a workspace names outside it, unless `runtimeOnFail` says
/// to go on with one that does not match.
pub(super) fn check_requires_python<Reporter: self::Reporter + 'static>(
    root: &Path,
    interpreter: &Interpreter,
    requires_python: Option<&pep440_rs::VersionSpecifiers>,
    mismatch: Mismatch,
) -> Result<()> {
    let version = interpreter.target.environment.python_full_version();
    let Some(specifiers) = requires_python.filter(|specifiers| !specifiers.contains(version))
    else {
        return Ok(());
    };
    if accepted::<Reporter>(root, version, specifiers, mismatch) {
        return Ok(());
    }
    bail!("{} requires Python {specifiers}, but {version} was selected", root.display())
}
