use super::{CafsFileInfo, HashMap, StoreDir, VerifiedFilesCache, verify_file};
use rayon::prelude::*;

/// The on-disk check of one store-index row's files, split off its
/// map building by [`defer_pkg_files_integrity`](super::defer_pkg_files_integrity).
#[derive(Debug)]
pub struct PendingFilesCheck {
    pub(super) files: HashMap<String, CafsFileInfo>,
    pub(super) algo: String,
}

impl PendingFilesCheck {
    /// Whether every file the row records is still on disk with its
    /// recorded content. A file already in `verified_files_cache` is
    /// trusted without a stat; one that passes here is added to it.
    ///
    /// Every file is checked, so a run reports all of a row's problems.
    /// A row with many files checks them in parallel: each check is a
    /// stat, and on a network filesystem the stats of one package would
    /// otherwise queue behind each other for seconds.
    #[must_use]
    pub fn verify(self, store_dir: &StoreDir, verified_files_cache: &VerifiedFilesCache) -> bool {
        let check = |(filename, info): (&String, &CafsFileInfo)| {
            // A malformed digest already failed the row's maps.
            let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
                return false;
            };
            if verified_files_cache.contains(&path) {
                return true;
            }
            let verified = verify_file(&path, filename, info, &self.algo);
            if verified {
                verified_files_cache.insert(path);
            }
            verified
        };
        let failed = if self.files.len() < PARALLEL_FILES_CHECK_MIN {
            self.files
                .iter()
                .filter(|file| !check(*file))
                .count()
        } else {
            self.files
                .par_iter()
                .filter(|file| !check(*file))
                .count()
        };
        failed == 0
    }
}

/// From how many files a row's check runs them in parallel.
const PARALLEL_FILES_CHECK_MIN: usize = 32;
