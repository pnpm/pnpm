use super::{ImportIndexedDirError, Placement, remove_non_dir_dirent, staging::import_atomic};
use crate::import_into_fresh_target;
use pnpm_config::PackageImportMethod;
use pnpm_reporter::Reporter;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

/// Make the parent dir set, then run the parallel per-entry import over
/// `cas_paths`. Mirrors pnpm v11's `tryImportIndexedDir`: collect the
/// unique relative parent dirs, sort shortest-first, mkdir each
/// sequentially, then dispatch the file imports in parallel. Sorting by
/// length means the recursive mkdir for a deeper dir always finds its
/// ancestor already on disk, so each call costs one `mkdirat` instead of
/// walking up.
pub(super) fn populate_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    placement: Placement,
) -> Result<(), ImportIndexedDirError> {
    create_indexed_dirs(dir_path, cas_paths, placement)?;

    // Link every other file first, then place the marker last, so an
    // interrupted import leaves a directory the next install recognises
    // as incomplete (pnpm's `tryImportIndexedDir`).
    let marker = marker_file(cas_paths);
    cas_paths
        .par_iter()
        .filter(|(cleaned_entry, _)| Some(cleaned_entry.as_str()) != marker)
        .try_for_each(|(cleaned_entry, store_path)| {
            place_entry::<Reporter>(
                placement,
                logged_methods,
                import_method,
                store_path,
                &dir_path.join(cleaned_entry),
            )
        })?;

    if let Some(marker) = marker {
        place_marker::<Reporter>(
            placement,
            logged_methods,
            import_method,
            &cas_paths[marker],
            &dir_path.join(marker),
        )?;
    }
    Ok(())
}
pub(super) fn create_indexed_dirs(
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    placement: Placement,
) -> Result<(), ImportIndexedDirError> {
    let mut rel_dirs: HashSet<&str> = HashSet::new();
    for entry in cas_paths.keys() {
        if let Some(parent) = Path::new(entry).parent()
            && let Some(rel) = parent.to_str()
            && !rel.is_empty()
        {
            rel_dirs.insert(rel);
        }
    }

    // The package root itself: pnpm's `importIndexedDir` mkdirs
    // `newDir` before calling `tryImportIndexedDir`, so do that here
    // too. Files at the package root (e.g. `package.json`) need this
    // even when `rel_dirs` is empty.
    fs::create_dir_all(dir_path).map_err(|error| ImportIndexedDirError::CreateDir {
        dirname: dir_path.to_path_buf(),
        error,
    })?;

    let mut ordered: Vec<&str> = rel_dirs.into_iter().collect();
    ordered.sort_by_key(|s| s.len());
    for rel in ordered {
        if placement == Placement::Repair {
            clear_dirent_blocking_dir(dir_path, rel)?;
        }
        let abs = dir_path.join(rel);
        fs::create_dir_all(&abs)
            .map_err(|error| ImportIndexedDirError::CreateDir { dirname: abs, error })?;
    }

    Ok(())
}
/// Put one indexed entry at `target`.
///
/// [`Placement::Fresh`] links straight at the final path with no
/// pre-flight stat: `import_into_fresh_target` tolerates an existing
/// target, so the stat would be pure overhead — ~170k saved syscalls on
/// the alotta-files fixture.
///
/// [`Placement::Repair`] instead asks whether what is already there is
/// this store entry, and swaps a fresh copy in when it is not. A
/// hardlinked or reflinked entry shares the store inode, so recognising
/// an intact file costs two stats and no read.
pub(super) fn place_entry<Reporter: self::Reporter>(
    placement: Placement,
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    store_path: &Path,
    target: &Path,
) -> Result<(), ImportIndexedDirError> {
    match placement {
        Placement::Fresh => {
            import_into_fresh_target::<Reporter>(logged_methods, import_method, store_path, target)
                .map_err(ImportIndexedDirError::LinkFile)
        }
        Placement::Repair => {
            if file_matches_store_entry(target, store_path) {
                return Ok(());
            }
            clear_dir_blocking_file(target)?;
            import_atomic::<Reporter>(logged_methods, import_method, store_path, target)
        }
    }
}
/// The completion marker is always placed atomically, in either
/// placement, so no reader observes it half-written. A repair adds the
/// clearing pass, since the marker path may hold a directory in a tree
/// damaged badly enough to need one.
pub(super) fn place_marker<Reporter: self::Reporter>(
    placement: Placement,
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    store_path: &Path,
    target: &Path,
) -> Result<(), ImportIndexedDirError> {
    if placement == Placement::Repair {
        clear_dir_blocking_file(target)?;
    }
    import_atomic::<Reporter>(logged_methods, import_method, store_path, target)
}
/// Remove a directory sitting where a package file belongs: the rename
/// in [`import_atomic`] replaces a file but never a directory.
pub(super) fn clear_dir_blocking_file(target: &Path) -> Result<(), ImportIndexedDirError> {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(target).map_err(|error| {
            ImportIndexedDirError::ClearBlockingDirEntry { path: target.to_path_buf(), error }
        }),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(ImportIndexedDirError::InspectTarget { path: target.to_path_buf(), error })
        }
    }
}
/// Remove any non-directory dirent along `rel`'s ancestry, so that the
/// `create_dir_all` which follows has somewhere to put the directory.
/// Walking top-down means a component whose parent is itself a file is
/// never stat-ed: the parent is cleared first, and everything below a
/// missing component is missing too.
pub(super) fn clear_dirent_blocking_dir(
    root: &Path,
    rel: &str,
) -> Result<(), ImportIndexedDirError> {
    let mut abs = root.to_path_buf();
    for component in Path::new(rel).components() {
        abs.push(component);
        match fs::symlink_metadata(&abs) {
            Ok(meta) if meta.is_dir() => {}
            Ok(meta) => remove_non_dir_dirent(&abs, meta.file_type()).map_err(|error| {
                ImportIndexedDirError::ClearBlockingDirEntry { path: abs.clone(), error }
            })?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(ImportIndexedDirError::InspectTarget { path: abs, error }),
        }
    }
    Ok(())
}
/// The completion-marker filename, mirroring pnpm's `pickFileFromFilesMap`:
/// `package.json` when present, else a fallback file for old store entries
/// indexed before the synthetic manifest. pnpm picks the first inserted
/// key; `cas_paths` is unordered, so we pick the lexicographically
/// smallest non-build-marker entry instead — deterministic, which is all
/// the gate and the write need. `None` only when no package file is present.
pub(super) fn marker_file(cas_paths: &HashMap<String, PathBuf>) -> Option<&str> {
    const PACKAGE_JSON: &str = "package.json";
    if cas_paths.contains_key(PACKAGE_JSON) {
        return Some(PACKAGE_JSON);
    }
    cas_paths.keys().map(String::as_str).filter(|path| *path != crate::NEEDS_BUILD_MARKER).min()
}
/// Whether `dir_path` already holds exactly this import, pnpm's
/// `allFilesMatch`. Existence is not enough: the completion marker goes
/// down last, but a file truncated by an interrupted copy or damaged
/// after the import finished still exists, and treating that slot as
/// done would leave it broken for every later install. The needs-build
/// marker is transient and does not identify package contents.
///
/// The marker is checked first, and `cas_paths` iterates in no
/// particular order: a slot another importer is still filling is the
/// common case here, and one `stat` settles it without comparing the
/// files that did land.
pub(super) fn all_files_match(dir_path: &Path, cas_paths: &HashMap<String, PathBuf>) -> bool {
    marker_present(dir_path, cas_paths)
        && cas_paths
            .iter()
            .filter(|(entry, _)| entry.as_str() != crate::NEEDS_BUILD_MARKER)
            .all(|(entry, store_path)| file_matches_store_entry(&dir_path.join(entry), store_path))
}
/// Whether `target` already carries `store_path`'s content. Imports that
/// hardlink or reflink share the store file, which settles it without a
/// read; the copy tier falls back to comparing size and then bytes, the
/// way pnpm's `allFilesMatch` does.
pub(super) fn file_matches_store_entry(target: &Path, store_path: &Path) -> bool {
    let (Ok(target_meta), Ok(store_meta)) =
        (fs::symlink_metadata(target), fs::metadata(store_path))
    else {
        return false;
    };
    if !target_meta.is_file() {
        return false;
    }
    // Unix carries the file's identity in the stat results already.
    // Windows keeps it behind an open handle, which `same-file` opens —
    // worth two handles to spare a hardlinked package a full read on the
    // platform where hardlinking is the default tier.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if target_meta.ino() == store_meta.ino() && target_meta.dev() == store_meta.dev() {
            return true;
        }
    }
    #[cfg(windows)]
    if same_file::is_same_file(target, store_path).unwrap_or(false) {
        return true;
    }
    target_meta.len() == store_meta.len()
        && files_have_equal_contents(target, store_path).unwrap_or(false)
}
/// Byte-compare two files without buffering either one.
///
/// [`populate_dir`] runs its entries through rayon, so a repair can be
/// comparing as many packages as there are workers at once. Reading
/// both sides whole would hold two allocations the size of the file per
/// worker, and a store entry for a native binary (`@napi-rs/*`,
/// `esbuild`) runs to tens of megabytes. Streaming holds one 8 KB
/// buffer per side instead, and stops at the first differing chunk
/// rather than reading two files that already disagree in byte one.
/// `pnpm_fs`'s `file_equals_bytes` streams for the same reason.
pub(super) fn files_have_equal_contents(left: &Path, right: &Path) -> io::Result<bool> {
    use std::io::BufRead;

    let mut left = io::BufReader::new(fs::File::open(left)?);
    let mut right = io::BufReader::new(fs::File::open(right)?);
    loop {
        let left_chunk = left.fill_buf()?;
        let right_chunk = right.fill_buf()?;
        // One side ending first means the sizes disagree after all —
        // the caller's size check can only read stale metadata.
        if left_chunk.is_empty() || right_chunk.is_empty() {
            return Ok(left_chunk.is_empty() && right_chunk.is_empty());
        }
        let len = left_chunk.len().min(right_chunk.len());
        if left_chunk[..len] != right_chunk[..len] {
            return Ok(false);
        }
        left.consume(len);
        right.consume(len);
    }
}
/// Whether `dir_path` holds the completion marker. An empty map has no
/// marker, so it counts as present — there is nothing to import.
#[must_use]
pub fn marker_present(dir_path: &Path, cas_paths: &HashMap<String, PathBuf>) -> bool {
    match marker_file(cas_paths) {
        Some(marker) => dir_path.join(marker).exists(),
        None => true,
    }
}
