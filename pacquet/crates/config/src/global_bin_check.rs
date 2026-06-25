//! Port of pnpm's
//! [`checkGlobalBinDir`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/checkGlobalBinDir.ts):
//! the global bin directory must exist on `PATH` (so the executables it
//! links are reachable) and, for mutating commands, be writable.

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Failure from [`check_global_bin_dir`]. Codes mirror pnpm's
/// `ERR_PNPM_`-prefixed `PnpmError` codes.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CheckGlobalBinDirError {
    /// The `PATH` environment variable is unset, so there is no global
    /// executables directory to validate against.
    #[display(
        "Couldn't find a global directory for executables because the \"PATH\" environment variable is not set."
    )]
    #[diagnostic(code(ERR_PNPM_NO_PATH_ENV))]
    NoPathEnv,

    /// The configured global bin directory is not one of the `PATH` entries.
    #[display("The configured global bin directory \"{}\" is not in PATH", global_bin_dir.display())]
    #[diagnostic(
        code(ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH),
        help("Run \"pnpm setup\" to update your shell configuration.")
    )]
    NotInPath {
        #[error(not(source))]
        global_bin_dir: PathBuf,
    },

    /// The global bin directory is missing or not writable by the CLI.
    #[display("The CLI has no write access to the global bin directory at {}", global_bin_dir.display())]
    #[diagnostic(code(ERR_PNPM_PNPM_DIR_NOT_WRITABLE))]
    NotWritable {
        #[error(not(source))]
        global_bin_dir: PathBuf,
    },
}

/// Validate that `global_bin_dir` is a usable global executables directory.
///
/// `path_env` is the value of `PATH` (the caller reads it so this stays
/// testable). When `should_allow_write` is set, the directory must also
/// exist and be writable — pnpm enforces this for every command except
/// `root`.
pub fn check_global_bin_dir(
    global_bin_dir: &Path,
    path_env: Option<&str>,
    should_allow_write: bool,
) -> Result<(), CheckGlobalBinDirError> {
    let Some(path_env) = path_env.filter(|value| !value.is_empty()) else {
        return Err(CheckGlobalBinDirError::NoPathEnv);
    };
    if !global_bin_dir_is_in_path(global_bin_dir, path_env) {
        return Err(CheckGlobalBinDirError::NotInPath {
            global_bin_dir: global_bin_dir.to_path_buf(),
        });
    }
    if should_allow_write && !can_write_to_dir_and_exists(global_bin_dir) {
        return Err(CheckGlobalBinDirError::NotWritable {
            global_bin_dir: global_bin_dir.to_path_buf(),
        });
    }
    Ok(())
}

fn global_bin_dir_is_in_path(global_bin_dir: &Path, path_env: &str) -> bool {
    let real_global_bin_dir = fs::canonicalize(global_bin_dir).ok();
    std::env::split_paths(path_env).any(|dir| {
        dirs_equal(global_bin_dir, &dir)
            || real_global_bin_dir.as_deref().is_some_and(|real| dirs_equal(real, &dir))
    })
}

/// Mirrors pnpm's `areDirsEqual` (`path.relative(dir1, dir2) === ''`).
fn dirs_equal(dir1: &Path, dir2: &Path) -> bool {
    dir1 == dir2
}

fn can_write_to_dir_and_exists(dir: &Path) -> bool {
    if !dir.exists() {
        return false;
    }
    let probe = dir.join(format!(".pacquet-write-probe-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckGlobalBinDirError, check_global_bin_dir};
    use std::path::Path;

    #[test]
    fn no_path_env_when_unset_or_empty() {
        let dir = Path::new("/some/bin");
        assert!(matches!(
            check_global_bin_dir(dir, None, false),
            Err(CheckGlobalBinDirError::NoPathEnv)
        ));
        assert!(matches!(
            check_global_bin_dir(dir, Some(""), false),
            Err(CheckGlobalBinDirError::NoPathEnv)
        ));
    }

    #[test]
    fn not_in_path_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let other = tmp.path().join("other").to_string_lossy().into_owned();
        let result = check_global_bin_dir(&bin, Some(&other), false);
        assert!(matches!(result, Err(CheckGlobalBinDirError::NotInPath { .. })));
    }

    #[test]
    fn ok_when_in_path() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let path_env = bin.to_string_lossy().into_owned();
        check_global_bin_dir(&bin, Some(&path_env), true).unwrap();
    }
}
