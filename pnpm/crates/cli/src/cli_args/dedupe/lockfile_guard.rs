use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// A drop guard for `--check` mode. The install pipeline hands back the
/// resolved lockfile instead of writing it, so the only writes left to undo are
/// the ones that run ahead of it, such as config-dependency syncing. Call
/// [`settle`](LockfileGuard::settle) once those have run. On drop the guard
/// undoes only its own command's write, so an edit made by another process
/// while the check runs is kept.
pub(crate) struct LockfileGuard {
    existing: Option<String>,
    lockfile_path: PathBuf,
    state: GuardState,
}

enum GuardState {
    /// The pre-pipeline steps have not finished, so any difference from the
    /// snapshot may be theirs.
    Unsettled,
    /// The pre-pipeline steps left the lockfile as it was.
    Untouched,
    /// The pre-pipeline steps left this content on disk.
    Wrote(Option<String>),
}

impl LockfileGuard {
    pub(crate) fn new(existing: Option<String>, lockfile_path: &Path) -> Self {
        Self { existing, lockfile_path: lockfile_path.to_path_buf(), state: GuardState::Unsettled }
    }

    /// Record what the steps before the install pipeline left on disk.
    pub(crate) fn settle(&mut self) {
        if let Ok(on_disk) = super::read_lockfile_snapshot(&self.lockfile_path) {
            self.state = if on_disk == self.existing {
                GuardState::Untouched
            } else {
                GuardState::Wrote(on_disk)
            };
        }
    }

    fn should_restore(&self) -> bool {
        let on_disk = super::read_lockfile_snapshot(&self.lockfile_path);
        match &self.state {
            GuardState::Untouched => false,
            GuardState::Unsettled => !on_disk.is_ok_and(|on_disk| on_disk == self.existing),
            GuardState::Wrote(written) => !on_disk.is_ok_and(|on_disk| &on_disk != written),
        }
    }
}

impl Drop for LockfileGuard {
    fn drop(&mut self) {
        if !self.should_restore() {
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
