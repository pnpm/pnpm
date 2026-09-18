//! Refusing an optimistic install when a lockfile still carries merge-conflict markers.

use super::{
    ErrorKind, OptimisticRepeatInstallCheck, Path, PathBuf, Read, file_mtime_from_metadata, fs,
    lockfile_modified_since,
};

#[derive(Clone, Copy)]
pub(crate) enum LockfileConflictCheckFailure {
    MergeConflict,
    Unsafe,
}

pub(crate) fn first_lockfile_requiring_conflict_safe_install(
    check: &OptimisticRepeatInstallCheck<'_>,
    last_validated_timestamp: i64,
) -> Option<(PathBuf, LockfileConflictCheckFailure)> {
    let shared_lockfile = check.workspace_root.join(check.config.wanted_lockfile_name());
    if let Some(failure) =
        lockfile_conflict_check_failure(&shared_lockfile, last_validated_timestamp)
    {
        return Some((shared_lockfile, failure));
    }
    if check.config.shares_one_lockfile() {
        return None;
    }
    for (root_dir, _) in check.project_manifests {
        let lockfile_path = root_dir.join(check.config.wanted_lockfile_name());
        if lockfile_path != shared_lockfile
            && let Some(failure) =
                lockfile_conflict_check_failure(&lockfile_path, last_validated_timestamp)
        {
            return Some((lockfile_path, failure));
        }
    }
    None
}

pub(crate) const CONFLICT_MARKER: &[u8] = b"<<<<<<<";

pub(crate) const LOCKFILE_CONFLICT_SCAN_BUFFER_SIZE: usize = 8 * 1024;

pub(crate) fn lockfile_conflict_check_failure(
    path: &Path,
    last_validated_timestamp: i64,
) -> Option<LockfileConflictCheckFailure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(_) => return Some(LockfileConflictCheckFailure::Unsafe),
    };
    if !metadata.file_type().is_file() {
        return Some(LockfileConflictCheckFailure::Unsafe);
    }
    let Some(mtime) = file_mtime_from_metadata(&metadata) else {
        return Some(LockfileConflictCheckFailure::Unsafe);
    };
    if !lockfile_modified_since(mtime, last_validated_timestamp) {
        return None;
    }
    modified_lockfile_conflict_check_failure(path)
}

pub(crate) fn modified_lockfile_conflict_check_failure(
    path: &Path,
) -> Option<LockfileConflictCheckFailure> {
    let Some(mut file) = open_for_conflict_scan(path) else {
        return Some(LockfileConflictCheckFailure::Unsafe);
    };
    // The scan streams the whole file through one buffer, whatever its
    // size: every changed lockfile that passes it is parsed in full next,
    // so a size budget here could only refuse what the parse would read
    // anyway.
    let mut buffer = [0; LOCKFILE_CONFLICT_SCAN_BUFFER_SIZE + CONFLICT_MARKER.len() - 1];
    let mut carried = 0;
    loop {
        let read =
            match file.read(&mut buffer[carried..carried + LOCKFILE_CONFLICT_SCAN_BUFFER_SIZE]) {
                Ok(0) => return None,
                Ok(read) => read,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return Some(LockfileConflictCheckFailure::Unsafe),
            };
        let end = carried + read;
        if chunk_contains_marker(&buffer[..end]) {
            return Some(LockfileConflictCheckFailure::MergeConflict);
        }
        // Carry the longest prefix a marker could still straddle into the
        // next read.
        carried = end.min(CONFLICT_MARKER.len() - 1);
        buffer.copy_within(end - carried..end, 0);
    }
}

/// The lockfile, opened for the conflict scan. `None` for anything the scan
/// cannot read to a verdict: a missing or unreadable path, or a non-file.
fn open_for_conflict_scan(path: &Path) -> Option<fs::File> {
    let file = fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    metadata
        .file_type()
        .is_file()
        .then_some(file)
}

fn chunk_contains_marker(bytes: &[u8]) -> bool {
    bytes
        .windows(CONFLICT_MARKER.len())
        .any(|window| window == CONFLICT_MARKER)
}
