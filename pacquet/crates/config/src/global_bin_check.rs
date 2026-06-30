//! Port of pnpm's
//! [`checkGlobalBinDir`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/checkGlobalBinDir.ts):
//! the global bin directory must exist on `PATH` (so the executables it
//! links are reachable) and, for mutating commands, be writable.

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Failure from [`check_global_bin_dir`]. Codes mirror pnpm's
/// `ERR_PNPM_`-prefixed `PnpmError` codes.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CheckGlobalBinDirError {
    /// The `PATH` environment variable is unset, so there is no global
    /// executables directory to validate against.
    #[display(
        r#"Couldn't find a global directory for executables because the "PATH" environment variable is not set."#
    )]
    #[diagnostic(code(ERR_PNPM_NO_PATH_ENV))]
    NoPathEnv,

    /// The configured global bin directory is not one of the `PATH` entries.
    #[display(r#"The configured global bin directory "{}" is not in PATH"#, global_bin_dir.display())]
    #[diagnostic(
        code(ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH),
        help(r#"Run "pnpm setup" to update your shell configuration."#)
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

/// Mirrors pnpm's `areDirsEqual` (`path.relative(dir1, dir2) === ''`), which
/// normalizes both paths before comparing. Plain `Path` equality would treat
/// e.g. `/a/b/.` or `/a/x/../b` as different from `/a/b` and reject a global
/// bin dir that is effectively on `PATH`.
fn dirs_equal(dir1: &Path, dir2: &Path) -> bool {
    lexically_normalize(dir1) == lexically_normalize(dir2)
}

/// Collapse `.` / `..` / redundant separators without touching the
/// filesystem (it never resolves symlinks — neither does Node's
/// `path.relative`).
fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn can_write_to_dir_and_exists(dir: &Path) -> bool {
    if !dir.exists() {
        return false;
    }
    // Probe with an exclusive create (`O_EXCL`) under an unpredictable name:
    // `create_new` never follows a symlink to (or truncates) an existing
    // file, so this cannot be turned into a file-clobber primitive when the
    // bin dir is shared or attacker-writable.
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    for _ in 0..5 {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let probe =
            dir.join(format!(".pacquet-write-probe-{}-{nanos:x}-{seq:x}", std::process::id()));
        match fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
                return true;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests;
