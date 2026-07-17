use crate::{Lockfile, extract_main_document};
use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};
use pipe_trait::Pipe;
use serde_saphyr::MessageFormatter;
use std::{
    env, fs,
    io::{self, ErrorKind},
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
    pub(super) fn parse_yaml(path: &Path, source: &serde_saphyr::Error) -> Self {
        Self::ParseYaml { path: path.to_path_buf(), reason: format_yaml_error(source) }
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

impl Lockfile {
    /// Load lockfile from the current directory.
    pub fn load_from_current_dir() -> Result<Option<Self>, LoadLockfileError> {
        let file_path =
            env::current_dir().map_err(LoadLockfileError::CurrentDir)?.join(Lockfile::FILE_NAME);
        Self::load_from_path(&file_path)
    }

    /// Load the *current* lockfile from
    /// `<virtual_store_dir>/lock.yaml`: the file records what pacquet
    /// actually materialized on the previous install and is diffed
    /// against the wanted lockfile to decide which snapshots can be
    /// skipped.
    ///
    /// Returns `Ok(None)` when the file is absent (a fresh install
    /// against an empty `node_modules`), treating ENOENT as `null`.
    /// Same parse / version-check path as the wanted lockfile, so a
    /// major-version mismatch surfaces as a parse error rather than
    /// silently dropping the file.
    pub fn load_current_from_virtual_store_dir(
        virtual_store_dir: &Path,
    ) -> Result<Option<Self>, LoadLockfileError> {
        let file_path = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
        Self::load_from_path(&file_path)
    }

    /// Load the wanted lockfile (`<dir>/pnpm-lock.yaml`) — a
    /// directory-addressed loader for callers that resolve into a
    /// directory other than the process's current one. Returns
    /// `Ok(None)` when the file is absent, same as
    /// [`Self::load_from_current_dir`].
    pub fn load_wanted_from_dir(dir: &Path) -> Result<Option<Self>, LoadLockfileError> {
        Self::load_from_path(&dir.join(Lockfile::FILE_NAME))
    }

    /// Whether `<dir>/pnpm-lock.yaml` would load as `Some`: the file
    /// exists and its main document is non-empty. The same absence
    /// rules as [`Self::load_wanted_from_dir`] (a missing file, an
    /// empty file, and an env-only combined document all count as
    /// absent) without paying for the YAML parse — only the read and
    /// the document split.
    ///
    /// Any read failure other than `NotFound` (permissions, invalid
    /// UTF-8, I/O) reports the file as present: an existing-but-
    /// unreadable lockfile must not be mistaken for a missing one —
    /// the regenerate-on-missing path would overwrite it — and the
    /// real load surfaces the underlying error when the contents are
    /// actually needed.
    #[must_use]
    pub fn wanted_exists_in_dir(dir: &Path) -> bool {
        match fs::read_to_string(dir.join(Lockfile::FILE_NAME)) {
            Ok(content) => !extract_main_document(&content).trim().is_empty(),
            Err(error) => error.kind() != ErrorKind::NotFound,
        }
    }

    fn load_from_path(file_path: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let content = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return error.pipe(LoadLockfileError::ReadFile).pipe(Err),
        };
        let main = extract_main_document(&content);
        if main.trim().is_empty() {
            return Ok(None);
        }
        serde_saphyr::from_str::<Self>(main)
            .map(|mut lockfile| {
                lockfile.reconstruct_missing_directory_resolutions();
                Some(lockfile)
            })
            .map_err(|source| LoadLockfileError::parse_yaml(file_path, &source))
    }
}

#[cfg(test)]
mod tests;
