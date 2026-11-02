use super::{CafsFileInfo, FilesMap, HashMap, StoreDir, VerifiedFilesCache, verify_file};
use rayon::prelude::*;
use std::path::PathBuf;

/// The on-disk check of one store-index row's files, split off its
/// map building by [`defer_pkg_files_integrity`](super::defer_pkg_files_integrity).
#[derive(Debug)]
pub struct PendingFilesCheck {
    pub(super) files: HashMap<String, CafsFileInfo>,
    pub(super) algo: String,
}

/// The modes a passed [`PendingFilesCheck`] read, by CAFS path.
#[derive(Debug, Default)]
pub struct VerifiedModes(HashMap<PathBuf, u32>);

impl VerifiedModes {
    /// Record each mode on the entry of `files` at the same CAFS path.
    pub fn apply(&self, files: &mut FilesMap) {
        if self.0.is_empty() {
            return;
        }
        for file in files.values_mut() {
            if let Some(mode) = self.0.get(&file.path) {
                file.set_mode(*mode);
            }
        }
    }
}

impl PendingFilesCheck {
    /// The modes of the row's files when every file the row records is
    /// still on disk with its recorded content, `None` otherwise. A file
    /// already in `verified_files_cache` is trusted without a stat and
    /// reports the mode cached with it; one that passes here is added to it.
    ///
    /// Every file is checked, so a run reports all of a row's problems.
    /// A row with many files checks them in parallel: each check is a
    /// stat, and on a network filesystem the stats of one package would
    /// otherwise queue behind each other for seconds.
    #[must_use]
    pub fn verify(
        self,
        store_dir: &StoreDir,
        verified_files_cache: &VerifiedFilesCache,
    ) -> Option<VerifiedModes> {
        let check = |(filename, info): (&String, &CafsFileInfo)| {
            // A malformed digest already failed the row's maps.
            let path = store_dir.cas_file_path_by_mode(&info.digest, info.mode)?;
            if let Some(mode) = verified_files_cache.get(&path).map(|entry| *entry) {
                return Some((path, mode));
            }
            let mode = verify_file(&path, filename, info, &self.algo)?;
            verified_files_cache.insert(path.clone(), mode);
            Some((path, mode))
        };
        let checked: Vec<Option<(PathBuf, Option<u32>)>> =
            if self.files.len() < PARALLEL_FILES_CHECK_MIN {
                self.files.iter().map(check).collect()
            } else {
                self.files
                    .par_iter()
                    .map(check)
                    .collect()
            };
        // Off Unix no check reads a mode, so the map stays empty.
        let mut modes = HashMap::with_capacity(if cfg!(unix) { checked.len() } else { 0 });
        for file in checked {
            if let (path, Some(mode)) = file? {
                modes.insert(path, mode);
            }
        }
        Some(VerifiedModes(modes))
    }
}

/// From how many files a row's check runs them in parallel.
const PARALLEL_FILES_CHECK_MIN: usize = 32;
