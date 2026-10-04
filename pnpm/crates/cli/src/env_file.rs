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
//! environment is never overridden. A leading UTF-8 byte-order mark is
//! tolerated. A missing or malformed file fails the
//! command loudly rather than running with a partial environment. Parse
//! errors name the file but never echo its contents: env files routinely
//! hold secrets that must not leak into stderr and CI logs.
//!
//! The load runs on the main thread before the startup thread exists, the
//! only place `std::env::set_var` may run. `--env-file` is this binary's
//! own option: [`strip_flags`] removes its tokens from the argv a
//! dispatched pnpm receives (an older pnpm would reject the unknown flag),
//! and the loaded variables travel by environment inheritance instead.
//! Tokens past the passthrough point — script arguments after `--`, or
//! after a `run`/`exec` script name — are left alone (see
//! `crate::parse_boundary`).

use crate::{
    flag_relocation::ArgTable,
    parse_boundary::{option_width, passthrough_from, union_arity},
};
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
            // `fs::read` yields raw bytes and never fails with
            // `InvalidData`, so this is `read_line` rejecting non-UTF-8:
            // the file is readable but malformed, hence a redacted parse
            // error rather than a read error.
            dotenvy::Error::Io(source) if source.kind() == std::io::ErrorKind::InvalidData => {
                Self::Parse {
                    path: path.to_owned(),
                    message: "file contains invalid UTF-8".to_string(),
                }
            }
            dotenvy::Error::Io(source) => Self::Read { path: path.to_owned(), source },
            // dotenvy reports the offending line; only its position is
            // repeated here, so a malformed line cannot leak a secret into
            // stderr and CI logs.
            dotenvy::Error::LineParse(_, index) => Self::Parse {
                path: path.to_owned(),
                message: format!("invalid dotenv entry (error at index {index})"),
            },
            error => Self::Parse { path: path.to_owned(), message: error.to_string() },
        }
    }

    /// A key or value with an embedded NUL byte, which `std::env::set_var`
    /// would panic on instead of failing the command. The key is omitted
    /// from the message: it echoes env-file contents into stderr.
    fn nul(path: &Path) -> Self {
        Self::Parse { path: path.to_owned(), message: "value contains a NUL byte".to_string() }
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

/// Scan `argv` for `--env-file` paths and load them. Runs on the main
/// thread before the startup thread exists (see [`crate::main`]), which is
/// what makes the `set_var` calls in [`load`] sound. Best-effort by
/// necessity: around an option this binary does not know, the width
/// computation can only guess, but such a command line is rejected or
/// forwarded to another pnpm anyway.
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

/// The UTF-8 byte-order mark Windows editors routinely prepend to a `.env`
/// file. `dotenvy::from_path_iter` never runs the BOM removal that
/// `Iter::load` performs, so without this the first key would carry a
/// leading U+FEFF and fail to parse (or misname the variable).
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn load_one(path: &Path) -> miette::Result<()> {
    let bytes =
        std::fs::read(path).map_err(|source| EnvFileError::Read { path: path.to_owned(), source })?;
    let stripped = bytes.strip_prefix(&UTF8_BOM).unwrap_or(&bytes);
    for pair in dotenvy::from_read_iter(stripped) {
        let (key, value) = pair.map_err(|error| EnvFileError::load(path, error))?;
        // `set_var` panics on a NUL byte in either half; `var_os` would
        // report a NUL key as merely absent, so the guard below cannot be
        // relied on to catch it.
        if key.contains('\0') || value.contains('\0') {
            return Err(EnvFileError::nul(path).into());
        }
        if std::env::var_os(&key).is_none() {
            // SAFETY: `load` runs on the main thread before `run_on_big_stack`
            // spawns the startup thread, so no other thread exists that
            // could access the environment concurrently.
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
/// token only when the token cannot spell an option (see
/// [`claimable_value`]); a valueless `--env-file` stays in place for clap
/// to report. The `--env-file=<path>` form always carries its own value.
fn scan(argv: &[OsString]) -> (Vec<PathBuf>, Vec<OsString>) {
    let passthrough = child_argv_boundary(argv);
    let arity = union_arity();
    let mut paths = Vec::new();
    let mut remaining = Vec::with_capacity(argv.len());
    let mut index = 0;
    while index < argv.len() {
        if passthrough.is_some_and(|boundary| index >= boundary) {
            remaining.extend(argv[index..].iter().cloned());
            break;
        }
        if let Some((path, width)) = claim_env_file(argv, index, passthrough) {
            paths.push(path);
            index += width;
        } else {
            let end =
                index + token_width(&argv[index], argv.get(index + 1), arity, argv.len() - index);
            remaining.extend(argv[index..end].iter().cloned());
            index = end;
        }
    }
    (paths, remaining)
}

/// The first argv index that must reach the child untouched, over an argv
/// that still carries the program name at index 0.
///
/// `pm` forces the built-in command but is not a subcommand of its own, so
/// the boundary is computed as if the prefix were absent and shifted back
/// afterwards. Without that, everything after a leading `pm` would look
/// forwarded and `--env-file` would never be stripped.
fn child_argv_boundary(argv: &[OsString]) -> Option<usize> {
    let pm_prefix = usize::from(
        argv.get(1)
            .is_some_and(|token| token == "pm"),
    );
    passthrough_from(&argv[pm_prefix..]).map(|boundary| boundary + pm_prefix)
}

/// The `--env-file` path named at `index`, with the number of argv slots
/// the flag occupies, or `None` when the token at `index` is not pnpm's
/// `--env-file`.
fn claim_env_file(
    argv: &[OsString],
    index: usize,
    passthrough: Option<usize>,
) -> Option<(PathBuf, usize)> {
    let text = argv[index].to_str()?;
    if let Some(path) = text.strip_prefix("--env-file=") {
        return Some((PathBuf::from(path), 1));
    }
    if text != "--env-file" {
        return None;
    }
    let value = argv
        .get(index + 1)
        .filter(|next| claimable_value(next))?;
    if passthrough.is_some_and(|boundary| index + 1 >= boundary) {
        return None;
    }
    Some((PathBuf::from(value), 2))
}

/// Whether `next` can be a bare `--env-file` value: anything but another
/// option. A non-UTF-8 token cannot spell an option, so it is claimable
/// (clap parses it into the path all the same).
fn claimable_value(next: &OsString) -> bool {
    next.to_str()
        .is_none_or(|text| !text.starts_with('-'))
}

/// The argv slots the token at `index` occupies: its option width, or one
/// slot for a positional or an unrecognized token. Clamped to `rest` so a
/// trailing value-taking option leaves its flag in place for clap to
/// report instead of running the scan past the end.
fn token_width(token: &OsString, next: Option<&OsString>, arity: &ArgTable, rest: usize) -> usize {
    token
        .to_str()
        .and_then(|text| option_width(text, next.and_then(|next| next.to_str()), arity))
        .unwrap_or(1)
        .min(rest)
}

#[cfg(test)]
mod tests;
