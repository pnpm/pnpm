//! Why a lockfile could not be loaded.

use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{self, Diagnostic};
use serde_saphyr::MessageFormatter;
use std::{
    io,
    path::{Path, PathBuf},
};

/// Error when reading lockfile the filesystem.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadLockfileError {
    #[display("Failed to get current_dir: {_0}")]
    #[diagnostic(code(ERR_PNPM_LOCKFILE_CURRENT_DIR))]
    CurrentDir(io::Error),

    #[display("Failed to read lockfile content: {_0}")]
    #[diagnostic(code(ERR_PNPM_LOCKFILE_READ_FILE))]
    ReadFile(io::Error),

    #[display(
        "The lockfile at \"{}\" is broken: {}",
        path.display(),
        reason
    )]
    #[diagnostic(code(ERR_PNPM_BROKEN_LOCKFILE))]
    ParseYaml { path: PathBuf, reason: String },
}

impl LoadLockfileError {
    pub(crate) fn parse_yaml(path: &Path, source: &serde_saphyr::Error) -> Self {
        Self::ParseYaml { path: path.to_path_buf(), reason: format_yaml_error(source) }
    }

    /// A next-format document that parsed but describes no lockfile.
    pub(crate) fn next_format(path: &Path, source: &crate::NextFormatError) -> Self {
        Self::ParseYaml { path: path.to_path_buf(), reason: source.to_string() }
    }
}

fn format_yaml_error(error: &serde_saphyr::Error) -> String {
    let error = error.without_snippet();
    let reason = serde_saphyr::DefaultMessageFormatter.format_message(error);
    if let Some(location) = error.location() {
        format!("{reason} ({}:{})", location.line(), location.column())
    } else {
        reason.into_owned()
    }
}
