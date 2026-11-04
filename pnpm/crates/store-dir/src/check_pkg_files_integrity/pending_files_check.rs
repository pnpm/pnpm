use super::{
    CafsFileInfo, FilesMap, HashMap, SideEffectsOverlay, StoreDir, VerifiedFilesCache, verify_file,
};
use rayon::prelude::*;
use std::path::Path;

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
    /// Records the mode of each file that passes on its entry in every map
    /// of `maps` that names the same store file.
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
        maps: &[&FilesMap],
    ) -> bool {
        let check = |(filename, info): (&String, &CafsFileInfo)| -> bool {
            // A malformed digest already failed the row's maps.
            let Some(path) = store_dir.cas_file_path_by_mode(&info.digest, info.mode) else {
                return false;
            };
            let cached = verified_files_cache.get(&path).map(|entry| *entry);
            let mode = if let Some(mode) = cached {
                mode
            } else {
                let Some(mode) = verify_file(&path, filename, info, &self.algo) else {
                    return false;
                };
                verified_files_cache.insert(path.clone(), mode);
                mode
            };
            if let Some(mode) = mode {
                record_mode(maps, filename, &path, mode);
            }
            true
        };
        let failed = if self.files.len() < PARALLEL_FILES_CHECK_MIN {
            self.files
                .iter()
                .map(check)
                .filter(|passed| !passed)
                .count()
        } else {
            self.files
                .par_iter()
                .map(check)
                .filter(|passed| !passed)
                .count()
        };
        failed == 0
    }
}

/// `base` and the files of every overlay in `overlays`: the maps a row's
/// [`PendingFilesCheck::verify`] records modes on.
#[must_use]
pub fn files_maps<'a>(
    base: &'a FilesMap,
    overlays: Option<&'a HashMap<String, SideEffectsOverlay>>,
) -> Vec<&'a FilesMap> {
    std::iter::once(base)
        .chain(
            overlays
                .into_iter()
                .flat_map(HashMap::values)
                .map(|overlay| &*overlay.files),
        )
        .collect()
}

/// An overlay's entry under `filename` can be a build output at another
/// store path, so an entry takes the mode only when its path matches.
fn record_mode(maps: &[&FilesMap], filename: &str, path: &Path, mode: u32) {
    for files in maps {
        if let Some(file) = files.get(filename)
            && file.path == path
        {
            file.record_mode(mode);
        }
    }
}

/// From how many files a row's check runs them in parallel.
const PARALLEL_FILES_CHECK_MIN: usize = 32;
