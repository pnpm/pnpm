use crate::safe_join_modules_dir::InvalidDependencyAliasError;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::{ForceSymlinkOutcome, force_symlink_dir, force_symlink_dir_absolute};
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

    /// A hoisted package's name is not a valid npm package name, so the
    /// `<slot>/node_modules/<name>` target the hoist symlink would point
    /// at could escape the slot's `node_modules`. Surfaces pnpm's
    /// `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
    #[diagnostic(transparent)]
    InvalidAlias(#[error(source)] InvalidDependencyAliasError),
}

/// Create a `node_modules/<name>` symlink for a direct dependency.
pub fn symlink_package(
    symlink_target: &Path,
    symlink_path: &Path,
) -> Result<ForceSymlinkOutcome, SymlinkPackageError> {
    symlink_package_impl(symlink_target, symlink_path, false)
}

/// Like [`symlink_package`], but stores the symlink target as an absolute path.
pub fn symlink_package_absolute(
    symlink_target: &Path,
    symlink_path: &Path,
) -> Result<ForceSymlinkOutcome, SymlinkPackageError> {
    symlink_package_impl(symlink_target, symlink_path, true)
}

fn symlink_package_impl(
    symlink_target: &Path,
    symlink_path: &Path,
    absolute: bool,
) -> Result<ForceSymlinkOutcome, SymlinkPackageError> {
    let force_symlink = if absolute { force_symlink_dir_absolute } else { force_symlink_dir };
    force_symlink(symlink_target, symlink_path)
        .map_err(|error| SymlinkPackageError::SymlinkDir {
            symlink_target: symlink_target.to_path_buf(),
            symlink_path: symlink_path.to_path_buf(),
            error,
        })
}
