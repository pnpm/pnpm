//! Build the `PATH` a spawned child sees.

use derive_more::{Display, Error};
use pnpm_diagnostics::miette::Diagnostic;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    process::Command,
};

/// A directory that cannot be expressed as a single `PATH` entry, because
/// it contains the separator entries are split on. Each command that
/// prepends directories reports it under its own error type, so the
/// `ERR_PNPM_BAD_PATH_DIR` diagnostic keeps the command's own context.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Cannot add {dir:?} to PATH: it contains the {delimiter:?} path separator")]
#[diagnostic(code(ERR_PNPM_BAD_PATH_DIR))]
pub(crate) struct BadPathDir {
    #[error(not(source))]
    pub(crate) dir: String,
    pub(crate) delimiter: char,
}

/// Prepend `dirs` to the current process `PATH`, most significant first.
///
/// A directory holding the platform path delimiter is rejected rather than
/// written: it would silently split into several entries, and one of the
/// halves could name a directory somebody else can write to. Every command
/// that puts a directory of its own in front of the user's `PATH` — `exec`,
/// `dlx`, `with`, and the shim dispatcher — goes through here, so they
/// cannot drift apart on that.
pub(crate) fn prepend_dirs_to_path(dirs: &[PathBuf]) -> Result<OsString, BadPathDir> {
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut path = OsString::new();
    for dir in dirs {
        let displayed = dir.to_string_lossy();
        if displayed.contains(delimiter) {
            return Err(BadPathDir { dir: displayed.into_owned(), delimiter });
        }
        if !path.is_empty() {
            path.push(separator);
        }
        path.push(dir);
    }
    if let Some(current) = std::env::var_os("PATH")
        && !current.is_empty()
    {
        if !path.is_empty() {
            path.push(separator);
        }
        path.push(current);
    }
    Ok(path)
}

/// Give `cmd` the `PATH` built by [`prepend_dirs_to_path`].
///
/// The inherited key is dropped before the new one is inserted: Windows
/// environment names are case-insensitive, so a process that inherited
/// `Path` and is then given `PATH` would carry both, and which one the
/// child reads is unspecified. Every pnpm spawn site goes through here so
/// none of them can forget that.
pub(crate) fn set_command_path(cmd: &mut Command, path: &OsStr) {
    cmd.env_remove("PATH");
    cmd.env_remove("Path");
    cmd.env("PATH", path);
}

#[cfg(test)]
mod tests;
