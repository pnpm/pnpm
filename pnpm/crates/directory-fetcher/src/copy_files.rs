//! The file set of a copy of a local directory.

use crate::DirectoryFetcher;
use std::path::Path;

/// Which files a copy of a local directory holds: an injected dependency,
/// or a project `pnpm deploy` copies. Every writer of such a copy reads
/// its source through [`Self::fetcher`]: the install, the sync after
/// scripts, the edit watcher, and the dedicated-lockfile sync.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CopyFiles {
    /// The files `pnpm pack` would include.
    #[default]
    PackageFiles,
    /// Every file of the directory.
    AllFiles,
}

impl CopyFiles {
    /// The file set the `deployAllFiles` setting selects.
    #[must_use]
    pub const fn from_deploy_all_files(deploy_all_files: bool) -> Self {
        if deploy_all_files { CopyFiles::AllFiles } else { CopyFiles::PackageFiles }
    }

    /// A fetcher that lists `directory` as a copy holds it. Symlinks are
    /// listed by their own path, for the writer to recreate, and no file
    /// may resolve outside `directory`.
    #[must_use]
    pub fn fetcher(self, directory: &Path) -> DirectoryFetcher {
        DirectoryFetcher {
            directory: directory.to_path_buf(),
            include_only_package_files: self == CopyFiles::PackageFiles,
            resolve_symlinks: false,
            preserve_symlinks: true,
            allow_path_escape: false,
        }
    }
}
