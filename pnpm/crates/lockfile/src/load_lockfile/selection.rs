use std::path::Path;

use crate::Lockfile;

/// Which wanted-lockfile file an install reads and writes, and whether the
/// other branches' lockfiles are folded into it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WantedLockfileSelection {
    /// The file the install reads first and writes back:
    /// `pnpm-lock.yaml`, or the `pnpm-lock.<branch>.yaml` that
    /// `useGitBranchLockfile` picked.
    pub file_name: String,
    /// `mergeGitBranchLockfiles`: fold every `pnpm-lock.<branch>.yaml`
    /// next to the file that was read into the loaded lockfile. The
    /// install deletes them once it has written the merge back.
    pub merge_git_branch_lockfiles: bool,
    /// Branch lockfile files a detached HEAD reads before
    /// `file_name`: the lockfiles of the branches containing the
    /// checked-out commit, which is the closest thing to a current branch
    /// such a checkout has. Empty when HEAD is attached to a branch. The
    /// write target is unaffected, a branch containing HEAD need not have
    /// HEAD at its tip.
    pub branch_lockfile_candidates: Vec<String>,
}

impl Default for WantedLockfileSelection {
    fn default() -> Self {
        WantedLockfileSelection {
            file_name: Lockfile::FILE_NAME.to_owned(),
            merge_git_branch_lockfiles: false,
            branch_lockfile_candidates: Vec::new(),
        }
    }
}

impl WantedLockfileSelection {
    /// Whether any file the read would try holds a wanted lockfile.
    ///
    /// With no candidates this asks about `file_name` alone, mirroring the
    /// single-file check pnpm's `existsNonEmptyWantedLockfile` performs on
    /// a branch: a branch that has not been installed on yet reads as
    /// having no lockfile even when the shared one is on disk. A detached
    /// HEAD's candidates widen the question to the whole read order, where
    /// the shared lockfile is the last file tried.
    pub(crate) fn wanted_exists_on_disk(&self, dir: &Path) -> bool {
        if self.branch_lockfile_candidates.is_empty() {
            return Lockfile::wanted_exists(dir, &self.file_name);
        }
        self.read_order().any(|file_name| Lockfile::wanted_exists(dir, file_name))
    }

    /// The file names to try, most specific first.
    pub fn read_order(&self) -> impl Iterator<Item = &str> {
        let branch_file = (self.file_name != Lockfile::FILE_NAME).then_some(&*self.file_name);
        self.branch_lockfile_candidates
            .iter()
            .map(String::as_str)
            // The candidates are only ever set for a detached HEAD, where
            // `file_name` is the shared lockfile; the filter keeps a caller
            // that sets both from reading the same file twice.
            .filter(move |candidate| Some(*candidate) != branch_file)
            .chain(branch_file)
            .chain([Lockfile::FILE_NAME])
    }
}
