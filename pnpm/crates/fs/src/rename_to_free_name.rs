use crate::rename_with_retry;
use std::{
    io,
    path::{Path, PathBuf},
};

const MAX_ATTEMPTS: usize = 100;

/// Rename `src` to `dst`, or to `dst_1`, `dst_2`, and so on when that name is
/// taken, and return where it landed.
///
/// An occupied name is never overwritten, so whatever already sits there
/// survives. Returns `Ok(None)` when every candidate name is taken.
pub fn rename_to_free_name(src: &Path, dst: &Path) -> io::Result<Option<PathBuf>> {
    for attempt in 0..MAX_ATTEMPTS {
        let candidate = match attempt {
            0 => dst.to_path_buf(),
            _ => suffixed(dst, attempt),
        };
        if candidate.symlink_metadata().is_ok() {
            continue;
        }
        match rename_with_retry(src, &candidate) {
            Ok(()) => return Ok(Some(candidate)),
            // Another process took the name between the check and the rename.
            Err(_) if candidate.symlink_metadata().is_ok() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn suffixed(path: &Path, attempt: usize) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or_default()
        .to_os_string();
    name.push(format!("_{attempt}"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests;
