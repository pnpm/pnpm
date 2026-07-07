//! The one PATH-prepend implementation shared by every command that
//! spawns a child with extra bin dirs (`exec`, `dlx`, `with`, the
//! `pnpmExecCommand` re-exec). Callers map [`DelimiterInDir`] into
//! their own diagnostic type so each keeps its `ERR_PNPM_BAD_PATH_DIR`
//! error, and pass the current `PATH` in so the logic stays testable
//! without process-env access.
//!
//! Mirrors the TypeScript CLI: the delimiter guard from
//! `@pnpm/exec.commands`' `makeEnv` and the already-leading
//! short-circuit from `@pnpm/shell.path`'s `prependDirsToPath`.

use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

/// A directory to prepend contains the platform path delimiter, so it
/// cannot be expressed as a single `PATH` entry and would silently
/// split into several.
#[derive(Debug)]
pub(crate) struct DelimiterInDir {
    pub dir: String,
    pub delimiter: char,
}

pub(crate) const DELIMITER: char = if cfg!(windows) { ';' } else { ':' };

/// Prepend `dirs` to `current` (the inherited `PATH`), unless they
/// already lead it.
pub(crate) fn prepend_dirs_to_path(
    dirs: &[PathBuf],
    current: Option<OsString>,
) -> Result<OsString, DelimiterInDir> {
    for dir in dirs {
        if dir.to_string_lossy().contains(DELIMITER) {
            return Err(DelimiterInDir {
                dir: dir.to_string_lossy().into_owned(),
                delimiter: DELIMITER,
            });
        }
    }

    let sep: &OsStr = if cfg!(windows) { OsStr::new(";") } else { OsStr::new(":") };
    let mut prepend = OsString::new();
    for (index, dir) in dirs.iter().enumerate() {
        if index > 0 {
            prepend.push(sep);
        }
        prepend.push(dir);
    }

    let current = current.filter(|value| !value.is_empty());
    if let Some(current) = &current {
        let leading = {
            let mut prefix = prepend.clone();
            prefix.push(sep);
            prefix
        };
        if *current == prepend || current.as_encoded_bytes().starts_with(leading.as_encoded_bytes())
        {
            return Ok(current.clone());
        }
    }

    let mut out = prepend;
    if let Some(current) = current {
        if !out.is_empty() {
            out.push(sep);
        }
        out.push(current);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
