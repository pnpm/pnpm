use crate::LoadLockfileError;
use pipe_trait::Pipe;
use std::{
    fs,
    io::ErrorKind,
    path::Path,
};

/// The text and verified open-handle metadata of a lockfile file, or `None`
/// when it is absent. Every other read failure is an error: an existing-but-
/// unreadable lockfile must not be mistaken for a missing one.
///
/// Metadata is returned only when pre-read and post-read handle stats agree
/// with the exact byte count read, so concurrent in-place mutation during the
/// read drops the stat shortcut to `None`.
pub(super) fn read_lockfile_file(
    file_path: &Path,
) -> Result<Option<(String, Option<fs::Metadata>)>, LoadLockfileError> {
    let mut file = match fs::File::open(file_path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return error.pipe(LoadLockfileError::ReadFile).pipe(Err),
    };
    let before = file.metadata().ok();
    let mut content = String::new();
    std::io::Read::read_to_string(&mut file, &mut content).map_err(LoadLockfileError::ReadFile)?;
    let after = file.metadata().ok();
    let stable = stable_open_file_metadata(before.as_ref(), after, content.len());
    Ok(Some((content, stable)))
}

pub(super) fn read_lockfile_text(file_path: &Path) -> Result<Option<String>, LoadLockfileError> {
    Ok(read_lockfile_file(file_path)?.map(|(content, _)| content))
}

fn stable_open_file_metadata(
    before: Option<&fs::Metadata>,
    after: Option<fs::Metadata>,
    read_len: usize,
) -> Option<fs::Metadata> {
    let (before, after) = (before?, after?);
    let same_times = match (before.modified(), after.modified()) {
        (Ok(t1), Ok(t2)) => t1 == t2,
        _ => false,
    };
    let same_size =
        before.len() == after.len() && u64::try_from(read_len).ok() == Some(after.len());
    (same_size && same_times && same_platform_identity(before, &after)).then_some(after)
}

#[cfg(unix)]
fn same_platform_identity(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

#[cfg(not(unix))]
fn same_platform_identity(_before: &fs::Metadata, _after: &fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests;
