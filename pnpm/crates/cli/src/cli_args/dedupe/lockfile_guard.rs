use std::path::{Path, PathBuf};

/// A drop guard for `--check` mode. The install pipeline hands back the
/// resolved lockfile instead of writing it, so the only writes left to undo are
/// the ones that run ahead of it, such as config-dependency syncing. On drop it
/// puts the snapshot back if the file on disk no longer matches it, so an
/// unexpected error during deduplication still leaves the workspace as it was.
/// A file that already matches is left alone, keeping its inode and mtime.
pub(crate) struct LockfileGuard {
    existing: Option<String>,
    lockfile_path: PathBuf,
}

impl LockfileGuard {
    pub(crate) fn new(existing: Option<String>, lockfile_path: &Path) -> Self {
        Self { existing, lockfile_path: lockfile_path.to_path_buf() }
    }
}

impl Drop for LockfileGuard {
    fn drop(&mut self) {
        if super::read_lockfile_snapshot(&self.lockfile_path)
            .is_ok_and(|on_disk| on_disk == self.existing)
        {
            return;
        }
        match self.existing.take() {
            Some(ref old) => {
                let _ = pnpm_fs::write_atomic(&self.lockfile_path, old.as_bytes());
            }
            None => {
                let _ = std::fs::remove_file(&self.lockfile_path);
            }
        }
    }
}
