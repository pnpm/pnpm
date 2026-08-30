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

pub(crate) const MAX_LOCKFILE_CONFLICT_SCAN_BYTES: u64 = 16 * 1024 * 1024;

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
    if metadata.len() >= MAX_LOCKFILE_CONFLICT_SCAN_BYTES {
        return Some(LockfileConflictCheckFailure::Unsafe);
    }
    modified_lockfile_conflict_check_failure(path)
}

pub(crate) fn modified_lockfile_conflict_check_failure(
    path: &Path,
) -> Option<LockfileConflictCheckFailure> {
    let Ok(mut file) = fs::File::open(path) else {
        return Some(LockfileConflictCheckFailure::Unsafe);
    };
    let Ok(metadata) = file.metadata() else {
        return Some(LockfileConflictCheckFailure::Unsafe);
    };
    if !metadata.file_type().is_file() || metadata.len() >= MAX_LOCKFILE_CONFLICT_SCAN_BYTES {
        return Some(LockfileConflictCheckFailure::Unsafe);
    }

    let mut buffer = [0; LOCKFILE_CONFLICT_SCAN_BUFFER_SIZE + CONFLICT_MARKER.len() - 1];
    let mut carried = 0;
    let mut scanned = 0_u64;
    loop {
        let remaining = MAX_LOCKFILE_CONFLICT_SCAN_BYTES.saturating_sub(scanned);
        if remaining == 0 {
            return Some(LockfileConflictCheckFailure::Unsafe);
        }
        let read_capacity = LOCKFILE_CONFLICT_SCAN_BUFFER_SIZE.min(remaining as usize);
        match file.read(&mut buffer[carried..carried + read_capacity]) {
            Ok(0) => return None,
            Ok(read) => {
                scanned += read as u64;
                let end = carried + read;
                if buffer[..end]
                    .windows(CONFLICT_MARKER.len())
                    .any(|bytes| bytes == CONFLICT_MARKER)
                {
                    return Some(LockfileConflictCheckFailure::MergeConflict);
                }
                carried = end.min(CONFLICT_MARKER.len() - 1);
                buffer.copy_within(end - carried..end, 0);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Some(LockfileConflictCheckFailure::Unsafe),
        }
    }
}
