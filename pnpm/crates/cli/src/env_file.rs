//! Load dotenv-style files named by `--env-file` into the process environment.
//!
//! `pnpm --env-file <path> <command>` layers the `KEY=VALUE` pairs from each
//! named file into the environment before anything else runs, so the
//! variables are visible to config resolution (`PNPM_CONFIG_<KEY>` overrides
//! and `${VAR}` tokens in user-level `.npmrc` files), to lifecycle scripts,
//! and to any pnpm the command dispatches to. This mirrors what
//! `node --env-file` and `dotenv-cli` wrappers do for pnpm today, without
//! requiring the wrapper.
//!
//! The flag is repeatable and the files are processed in order. The first
//! value naming a variable wins, and a variable already present in the
//! environment is never overridden. A missing or malformed file fails the
//! command loudly rather than running with a partial environment.
//!
//! `--env-file` is this binary's own option: [`strip_flags`] removes its
//! tokens from the argv a dispatched pnpm receives (an older pnpm would
//! reject the unknown flag), and the loaded variables travel by environment
//! inheritance instead. Tokens past the passthrough point — script arguments
//! after `--`, or after a `run`/`exec` script name — are left alone (see
//! `crate::parse_boundary`).

use crate::parse_boundary::{option_width, passthrough_from, union_arity};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Raised when an `--env-file` file cannot be read or parsed.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EnvFileError {
    /// The env file could not be read.
    #[display("Failed to read env file '{}': {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_ENV_FILE_READ))]
    Read { path: PathBuf, source: std::io::Error },
    /// The env file contains a line that is neither empty, a comment, nor
    /// a `KEY=VALUE` pair.
    #[display("Failed to parse env file '{}': {message}", path.display())]
    #[diagnostic(code(ERR_PNPM_ENV_FILE_PARSE))]
    Parse { path: PathBuf, message: String },
}

impl EnvFileError {
    fn load(path: &Path, error: dotenvy::Error) -> Self {
        match error {
            dotenvy::Error::Io(source) => Self::Read { path: path.to_owned(), source },
            error => Self::Parse { path: path.to_owned(), message: error.to_string() },
        }
    }
}

/// Load each file in order, layering its variables into the process
/// environment. The first file naming a variable wins, and variables the
/// environment already defines are left untouched.
pub fn load(paths: &[PathBuf]) -> miette::Result<()> {
    for path in paths {
        load_one(path)?;
    }
    Ok(())
}

/// Scan `argv` for `--env-file` paths and load them. Best-effort fallback
/// for command lines clap rejected with `UnknownArgument`: the child argv
/// no longer carries the flag (see [`strip_flags`]), so the pnpm the
/// command dispatches to can only inherit variables this process loads
/// itself.
pub fn load_from_argv(argv: &[OsString]) -> miette::Result<()> {
    load(&scan(argv).0)
}

/// Remove the `--env-file` tokens from an argv that still carries the
/// program name at index 0 (the shape the entry point builds the child argv
/// from), keeping every other token — including tokens past the passthrough
/// point — in place.
pub fn strip_flags(argv: &[OsString]) -> Vec<OsString> {
    scan(argv).1
}

fn load_one(path: &Path) -> miette::Result<()> {
    let pairs = dotenvy::from_path_iter(path).map_err(|error| EnvFileError::load(path, error))?;
    for pair in pairs {
        let (key, value) = pair.map_err(|error| EnvFileError::load(path, error))?;
        if std::env::var_os(&key).is_none() {
            // SAFETY: `load` runs on the CLI startup path before the tokio
            // runtime and rayon pool start, so the process is
            // single-threaded and no other thread can be reading the
            // environment concurrently.
            unsafe {
                std::env::set_var(&key, &value);
            }
        }
    }
    Ok(())
}

/// The pure core of [`load_from_argv`] and [`strip_flags`]: the `--env-file`
/// paths in `argv`, in order, alongside `argv` with those tokens removed.
///
/// Tokens past the passthrough point belong to the child command line, not
/// to pnpm, so they are never claimed. A bare `--env-file` claims the next
/// token only when the token does not start with `-`, mirroring clap (which
/// would report the valueless flag instead); a valueless `--env-file` stays
/// in place for clap to report. The `--env-file=<path>` form always carries
/// its own value.
fn scan(argv: &[OsString]) -> (Vec<PathBuf>, Vec<OsString>) {
    let passthrough = passthrough_from(argv);
    let arity = union_arity();
    let mut paths = Vec::new();
    let mut remaining = Vec::with_capacity(argv.len());
    let mut index = 0;
    while index < argv.len() {
        if passthrough.is_some_and(|boundary| index >= boundary) {
            remaining.extend(argv[index..].iter().cloned());
            break;
        }
        let token = &argv[index];
        let text = token.to_str();
        if let Some(path) = text.and_then(|text| text.strip_prefix("--env-file=")) {
            paths.push(PathBuf::from(path));
            index += 1;
            continue;
        }
        if text == Some("--env-file")
            && let Some(path) = argv
                .get(index + 1)
                .and_then(|next| next.to_str())
                .filter(|next| !next.starts_with('-'))
                .filter(|_| passthrough.is_none_or(|boundary| index + 1 < boundary))
        {
            paths.push(PathBuf::from(path));
            index += 2;
            continue;
        }
        let width = text
            .and_then(|text| {
                option_width(
                    text,
                    argv.get(index + 1)
                        .and_then(|next| next.to_str()),
                    arity,
                )
            })
            .unwrap_or(1);
        remaining.extend(argv[index..index + width.min(argv.len() - index)].iter().cloned());
        index += width;
    }
    (paths, remaining)
}

#[cfg(test)]
mod tests;
