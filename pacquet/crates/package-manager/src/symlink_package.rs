use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::{ForceSymlinkOutcome, force_symlink_dir};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Error type for [`symlink_package`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum SymlinkPackageError {
    #[display("Failed to create directory at {dir:?}: {error}")]
    CreateParentDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to create symlink at {symlink_path:?} to {symlink_target:?}: {error}")]
    SymlinkDir {
        symlink_target: PathBuf,
        symlink_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Create a `node_modules/<name>` symlink for a direct dependency.
///
/// Wraps [`pacquet_fs::force_symlink_dir`] so the call site mirrors
/// pnpm's `symlinkDependency` (which calls `symlinkDir(target, link)`
/// with the library's default `{ overwrite: true }`). That means:
///
/// * Missing parent directories are created with `create_dir_all`.
/// * An existing symlink already pointing at `symlink_target` is
///   reused (no-op).
/// * An existing symlink pointing elsewhere is replaced.
/// * A regular file or non-empty directory squatting at
///   `symlink_path` is renamed to
///   `<parent>/.ignored_<basename>` and the symlink is created.
///
/// Returns the [`ForceSymlinkOutcome`], so callers can mirror pnpm's
/// `if ((await symlinkDependency(...)).reused) return` — the direct-dependency
/// linker only emits a `pnpm:root added` event for symlinks it actually
/// created, not for ones already pointing at the target.
pub fn symlink_package(
    symlink_target: &Path,
    symlink_path: &Path,
) -> Result<ForceSymlinkOutcome, SymlinkPackageError> {
    // `force_symlink_dir` handles missing parent dirs via its own
    // `NotFound` retry that calls `create_dir_all` once and reissues
    // the symlink syscall. Pre-creating the parent here would just
    // pay an extra `stat` per symlink (~3-5k per install on the
    // alotta-files fixture) for the common case of a parent that
    // already exists from a prior `import_indexed_dir` populate or
    // sibling symlink.
    force_symlink_dir(symlink_target, symlink_path).map_err(|error| {
        SymlinkPackageError::SymlinkDir {
            symlink_target: symlink_target.to_path_buf(),
            symlink_path: symlink_path.to_path_buf(),
            error,
        }
    })
}
