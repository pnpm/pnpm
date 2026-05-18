use crate::{Lockfile, extract_main_document};
use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};
use pipe_trait::Pipe;
use std::{
    env, fs,
    io::{self, ErrorKind},
    path::Path,
};

/// Error when reading lockfile the filesystem.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadLockfileError {
    #[display("Failed to get current_dir: {_0}")]
    #[diagnostic(code(pacquet_lockfile::current_dir))]
    CurrentDir(io::Error),

    #[display("Failed to read lockfile content: {_0}")]
    #[diagnostic(code(pacquet_lockfile::read_file))]
    ReadFile(io::Error),

    #[display("Failed to parse lockfile content as YAML: {_0}")]
    #[diagnostic(code(pacquet_lockfile::parse_yaml))]
    ParseYaml(serde_saphyr::Error),
}

impl Lockfile {
    /// Load lockfile from the current directory.
    pub fn load_from_current_dir() -> Result<Option<Self>, LoadLockfileError> {
        let file_path =
            env::current_dir().map_err(LoadLockfileError::CurrentDir)?.join(Lockfile::FILE_NAME);
        Self::load_from_path(&file_path)
    }

    /// Load the *current* lockfile from
    /// `<virtual_store_dir>/lock.yaml`. Mirrors upstream's
    /// `readCurrentLockfile` at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/read.ts#L29-L37>:
    /// the file records what pacquet actually materialized on the
    /// previous install and is diffed against the wanted lockfile to
    /// decide which snapshots can be skipped.
    ///
    /// Returns `Ok(None)` when the file is absent (a fresh install
    /// against an empty `node_modules`), matching upstream's
    /// ENOENT-as-`null` semantics. Same parse / version-check path as
    /// the wanted lockfile, so a major-version mismatch surfaces as a
    /// parse error rather than silently dropping the file.
    pub fn load_current_from_virtual_store_dir(
        virtual_store_dir: &Path,
    ) -> Result<Option<Self>, LoadLockfileError> {
        let file_path = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
        Self::load_from_path(&file_path)
    }

    fn load_from_path(file_path: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let content = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return error.pipe(LoadLockfileError::ReadFile).pipe(Err),
        };
        // Skip the env lockfile document if present (first document in
        // pnpm v11's combined format). Mirrors upstream `_read` at
        // <https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/read.ts#L103-L110>:
        // an empty main document (env-only file) is treated as if the
        // lockfile is absent.
        let main = extract_main_document(&content);
        if main.trim().is_empty() {
            return Ok(None);
        }
        serde_saphyr::from_str(main).map(Some).map_err(LoadLockfileError::ParseYaml)
    }
}

#[cfg(test)]
mod tests;
