use crate::{
    LinkFileError, import_into_fresh_target,
    remove_quarantine::remove_quarantine_from_native_binaries,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::PackageImportMethod;
use pnpm_reporter::Reporter;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Options for [`import_indexed_dir`].
///
/// Mirrors pnpm v11's `ImportOptions` at
/// `store/controller-types/src/index.ts` for the fields pacquet
/// consumes today. The defaults match the isolated linker's call
/// shape (no force, no nested-modules preservation); the hoisted
/// linker passes both flags set to `true`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImportIndexedDirOpts {
    /// When `true`, re-import even when `dir_path` already exists,
    /// overwriting the existing contents. Without `force`, an
    /// existing directory short-circuits this function (matches
    /// pnpm's pre-existence check in `importIndexedPackage`).
    pub force: bool,
    /// When `true` (only meaningful with `force`), preserve
    /// `dir_path/node_modules/` across the re-import so nested
    /// dependencies survive the rebuild. Required by the hoisted
    /// linker, whose orphan-removal and insert passes are
    /// interleaved across the package tree — a nested `node_modules/`
    /// installed by a sibling pass must not be clobbered when the
    /// parent package is re-imported.
    pub keep_modules_dir: bool,
    /// Whether an occupied, complete target is equivalent to this import.
    ///
    /// Callers must ensure that the target path uniquely identifies its contents.
    pub safe_to_skip: bool,
}

/// Error type for [`import_indexed_dir`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ImportIndexedDirError {
    #[display("cannot create directory at {dirname:?}: {error}")]
    CreateDir {
        dirname: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[diagnostic(transparent)]
    LinkFile(#[error(source)] LinkFileError),
    #[display("failed to inspect existing target {path:?}: {error}")]
    InspectTarget {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to clear non-directory dirent at {path:?}: {error}")]
    ClearNonDirEntry {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display(
        "failed to move existing {from:?} into staging directory {to:?} while preserving node_modules: {error}"
    )]
    PreserveModulesDir {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to remove existing directory {path:?} prior to swap: {error}")]
    RemoveExisting {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to rename staging directory {from:?} to {to:?}: {error}")]
    Swap {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to place imported file {from:?} at {to:?}: {error}")]
    PlaceFile {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to clear {path:?}, which blocks the repair of a partial import: {error}")]
    ClearBlockingDirEntry {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// How [`populate_dir`] puts each indexed entry at its final path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// Nothing of this import is at the target yet: link straight at the
    /// final path and adopt a dirent a concurrent importer placed first,
    /// since the two are importing the same content-addressed file.
    Fresh,
    /// The target already holds part of an import: replace whatever does
    /// not match the store entry, so a file damaged or truncated by an
    /// interrupted import is healed rather than adopted.
    Repair,
}

enum PreservedModules {
    None,
    Directory,
    Merged { backup: PathBuf, moved_entries: Vec<OsString> },
}

impl PreservedModules {
    fn has_moved_data(&self) -> bool {
        match self {
            PreservedModules::None => false,
            PreservedModules::Directory => true,
            PreservedModules::Merged { .. } => true,
        }
    }
}

struct PreserveModulesFailure {
    error: io::Error,
    preserved: PreservedModules,
}

impl Placement {
    /// How much an import into an occupied target may assume about what
    /// is already there, given whether the target is shared.
    fn for_target(safe_to_skip: bool) -> Self {
        if safe_to_skip { Placement::Repair } else { Placement::Fresh }
    }
}

/// Materialize an indexed package's files into `dir_path`, the way
/// pnpm v11's `importIndexedDir` does at
/// `fs/indexed-pkg-importer/src/importIndexedDir.ts`. The same function
/// services both node-linkers; behavior at the destination is
/// controlled by [`ImportIndexedDirOpts`].
///
/// Files in `cas_paths` are materialized by `import_into_fresh_target()`
/// using `import_method`'s preference order
/// (hardlink → reflink → copy, etc.), and the per-method
/// `pnpm:package-import-method` log is emitted via `logged_methods`
/// the first time each tier is used in this install. The pre-flight
/// `fs::metadata` short-circuit lives on `link_file()`; an import into a
/// private target skips it and relies on `import_into_fresh_target`'s
/// EEXIST tolerance, which is what keeps a marker-repair re-link over a
/// partial directory correct.
pub fn import_indexed_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    opts: ImportIndexedDirOpts,
) -> Result<(), ImportIndexedDirError> {
    let existing_kind = match fs::symlink_metadata(dir_path) {
        Ok(meta) => Some(meta.file_type()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(ImportIndexedDirError::InspectTarget {
                path: dir_path.to_path_buf(),
                error,
            });
        }
    };

    // Drop the macOS quarantine xattr from the package's native binaries after
    // a populating import, matching pnpm's `removeQuarantineFromNativeBinaries`.
    // The marker-present short-circuit (and the non-directory dirent left as-is)
    // import nothing, so they skip the sweep, keeping warm installs free of the
    // per-install `xattr` cost — exactly pnpm's `!pkgExistsAtTargetDir` gate.
    let unquarantine = || remove_quarantine_from_native_binaries(dir_path, cas_paths);
    match (existing_kind, opts.force) {
        // An absent shared target says nothing: another importer can create it
        // between the stat above and the first file written below, and then two
        // importers are populating one directory each believing it owns it.
        // Ownership is settled by an exclusive `mkdir` instead.
        (None, _) if opts.safe_to_skip => {
            import_into_shared_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
                .inspect(|()| unquarantine())
        }
        (None, _) => populate_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            Placement::Fresh,
        )
        .inspect(|()| unquarantine()),
        // Short-circuit only when the completion marker is present
        // (pnpm's `pkgExistsAtTargetDir`, which checks `package.json`),
        // not on mere directory existence. A marker-less directory is a
        // partial import; repair it by re-running the non-destructive
        // `populate_dir`. Ported from pnpm/pnpm#12204 (cbfeeef328).
        //
        // Whose partial import it is decides how much the repair may
        // assume: a private target holds this install's own interrupted
        // work, so an existing dirent is this package's file and is
        // adopted, while a shared one may hold a file an importer died
        // halfway through writing, which only a replacement heals.
        (Some(file_type), false) if file_type.is_dir() => {
            if marker_present(dir_path, cas_paths) {
                Ok(())
            } else {
                populate_dir::<Reporter>(
                    logged_methods,
                    import_method,
                    dir_path,
                    cas_paths,
                    Placement::for_target(opts.safe_to_skip),
                )
                .inspect(|()| unquarantine())
            }
        }
        // A non-directory dirent is left as-is; only force=true clobbers it.
        (Some(_), false) => Ok(()),
        // Existing non-directory dirent with force=true. The hoisted
        // linker call shape won't produce this in practice, but
        // refusing to clobber a stale symlink would wedge the install.
        (Some(file_type), true) if !file_type.is_dir() => {
            remove_non_dir_dirent(dir_path, file_type).map_err(|error| {
                ImportIndexedDirError::ClearNonDirEntry { path: dir_path.to_path_buf(), error }
            })?;
            populate_dir::<Reporter>(
                logged_methods,
                import_method,
                dir_path,
                cas_paths,
                Placement::Fresh,
            )
            .inspect(|()| unquarantine())
        }
        // A forced refresh of a shared slot still works in place. Building a
        // complete stage first would duplicate every write before the rename
        // inevitably discovers that the shared directory already exists.
        (Some(file_type), true) if file_type.is_dir() && opts.safe_to_skip => {
            import_into_shared_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
                .inspect(|()| unquarantine())
        }
        (Some(_), true) => stage_and_swap::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            opts.keep_modules_dir,
        )
        .inspect(|()| unquarantine()),
    }
}

/// Import into a target whose path is shared with the installs running
/// in other projects, without ever removing anything from it.
///
/// The importer that creates the directory populates it; the others heal
/// what is there, because a slot that exists is either finished, being
/// filled right now, or left behind by an importer that died mid-file —
/// and the three are indistinguishable from the outside.
fn import_into_shared_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
) -> Result<(), ImportIndexedDirError> {
    if claim_dir(dir_path)? {
        return populate_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            Placement::Fresh,
        );
    }
    if all_files_match(dir_path, cas_paths) {
        return Ok(());
    }
    populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths, Placement::Repair)
}

/// Create `dir_path`, reporting whether this call is the one that created
/// it. `create_dir_all` cannot answer that — it succeeds either way.
fn claim_dir(dir_path: &Path) -> Result<bool, ImportIndexedDirError> {
    if let Some(parent) = dir_path.parent() {
        fs::create_dir_all(parent).map_err(|error| ImportIndexedDirError::CreateDir {
            dirname: parent.to_path_buf(),
            error,
        })?;
    }
    match fs::create_dir(dir_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => {
            Err(ImportIndexedDirError::CreateDir { dirname: dir_path.to_path_buf(), error })
        }
    }
}

/// Make the parent dir set, then run the parallel per-entry import over
/// `cas_paths`. Mirrors pnpm v11's `tryImportIndexedDir`: collect the
/// unique relative parent dirs, sort shortest-first, mkdir each
/// sequentially, then dispatch the file imports in parallel. Sorting by
/// length means the recursive mkdir for a deeper dir always finds its
/// ancestor already on disk, so each call costs one `mkdirat` instead of
/// walking up.
fn populate_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
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
fn place_entry<Reporter: self::Reporter>(
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
fn place_marker<Reporter: self::Reporter>(
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
fn clear_dir_blocking_file(target: &Path) -> Result<(), ImportIndexedDirError> {
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
fn clear_dirent_blocking_dir(root: &Path, rel: &str) -> Result<(), ImportIndexedDirError> {
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
fn marker_file(cas_paths: &HashMap<String, PathBuf>) -> Option<&str> {
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
fn all_files_match(dir_path: &Path, cas_paths: &HashMap<String, PathBuf>) -> bool {
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
fn file_matches_store_entry(target: &Path, store_path: &Path) -> bool {
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
fn files_have_equal_contents(left: &Path, right: &Path) -> io::Result<bool> {
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

/// Place a file atomically (pnpm's `importFileAtomic`): link it into a
/// private temp sibling, then rename onto `target` so it is never
/// observed half-written, and so a stale copy is replaced in one step
/// under a reader. pacquet picks its import tier at runtime, so it
/// always stages rather than predicting whether the import will copy.
/// A failed rename is accepted only when the destination now matches the
/// store entry, which means a concurrent importer won the race with the
/// same content.
fn import_atomic<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    store_path: &Path,
    target: &Path,
) -> Result<(), ImportIndexedDirError> {
    let temp = pick_stage_path(target);
    if let Err(error) =
        import_into_fresh_target::<Reporter>(logged_methods, import_method, store_path, &temp)
    {
        let _ = fs::remove_file(&temp);
        return Err(ImportIndexedDirError::LinkFile(error));
    }
    match pnpm_fs::rename_with_retry(&temp, target) {
        Ok(()) => Ok(()),
        Err(_) if file_matches_store_entry(target, store_path) => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(ImportIndexedDirError::PlaceFile { from: temp, to: target.to_path_buf(), error })
        }
    }
}

fn stage_and_swap<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    keep_modules_dir: bool,
) -> Result<(), ImportIndexedDirError> {
    let stage = pick_stage_path(dir_path);
    let modules_backup = pick_stage_path(dir_path);
    let target_modules = dir_path.join("node_modules");
    let stage_modules = stage.join("node_modules");

    // 1. Populate the staging directory with the new contents. On
    //    failure, the staging directory is the only thing on disk we
    //    own — a blanket rimraf is safe.
    if let Err(error) =
        populate_dir::<Reporter>(logged_methods, import_method, &stage, cas_paths, Placement::Fresh)
    {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    // 2. Inspect the existing `node_modules/` so nested deps survive
    //    the swap. Only `NotFound` is benign — `PermissionDenied` and
    //    other transient I/O failures must surface, otherwise the
    //    user's nested deps get silently clobbered when the directory
    //    is removed in step 4.
    let nm_kind = if keep_modules_dir {
        match fs::symlink_metadata(&target_modules) {
            Ok(meta) => Some(meta.file_type()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                let _ = fs::remove_dir_all(&stage);
                return Err(ImportIndexedDirError::InspectTarget { path: target_modules, error });
            }
        }
    } else {
        None
    };

    // 3. Preserve `node_modules/` if it's a real directory. A package
    //    may ship bundled dependencies, so merge non-conflicting
    //    top-level entries when the staged import already has its own
    //    `node_modules/`. The staged package wins conflicts, matching
    //    pnpm's `moveOrMergeModulesDirs`.
    let preserved_modules = match nm_kind {
        Some(file_type) if file_type.is_dir() => {
            match preserve_modules_dir(&target_modules, &stage_modules, &modules_backup) {
                Ok(preserved) => preserved,
                Err(PreserveModulesFailure { error, preserved }) => {
                    finalize_stage_cleanup_after_failure(
                        &preserved,
                        &stage,
                        &stage_modules,
                        &target_modules,
                    );
                    return Err(ImportIndexedDirError::PreserveModulesDir {
                        from: target_modules,
                        to: stage_modules,
                        error,
                    });
                }
            }
        }
        Some(_) | None => PreservedModules::None,
    };

    // 4. Remove the old contents. If this fails after step 3, the
    //    the staged tree and any merge backup hold the preserved data.
    //    Try to move it back into place before bailing, and retain
    //    those temporary paths if restoration can't run.
    if let Err(error) = pnpm_fs::remove_dir_all_with_retry(dir_path) {
        finalize_stage_cleanup_after_failure(
            &preserved_modules,
            &stage,
            &stage_modules,
            &target_modules,
        );
        return Err(ImportIndexedDirError::RemoveExisting { path: dir_path.to_path_buf(), error });
    }

    // 5. Move the staged tree into place. There's a brief window
    //    between `remove_dir_all` and `rename` where `dir_path` does
    //    not exist on disk — acceptable for a slot only this install
    //    can reach; a shared slot never enters this function. If the
    //    rename fails, recreate
    //    `dir_path` so the rescued `node_modules/` has somewhere to
    //    land.
    if let Err(error) = pnpm_fs::rename_with_retry(&stage, dir_path) {
        // `create_dir_all` is the gate: without `dir_path`, the rescue
        // rename has no destination. Treat its failure as "rescue
        // can't run" and leak the staging directory below.
        let rescue_target_ready =
            !preserved_modules.has_moved_data() || fs::create_dir_all(dir_path).is_ok();
        if rescue_target_ready {
            finalize_stage_cleanup_after_failure(
                &preserved_modules,
                &stage,
                &stage_modules,
                &target_modules,
            );
        } else {
            leak_stage(&stage, &stage_modules, &preserved_modules);
        }
        return Err(ImportIndexedDirError::Swap { from: stage, to: dir_path.to_path_buf(), error });
    }
    discard_replaced_modules(&preserved_modules);
    Ok(())
}

fn preserve_modules_dir(
    source: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<PreservedModules, PreserveModulesFailure> {
    match fs::rename(source, destination) {
        Ok(()) => return Ok(PreservedModules::Directory),
        Err(error) if is_modules_dir_collision(&error) => {}
        Err(error) => {
            return Err(PreserveModulesFailure { error, preserved: PreservedModules::None });
        }
    }

    fs::rename(source, backup)
        .map_err(|error| PreserveModulesFailure { error, preserved: PreservedModules::None })?;

    let destination_entries = fs::read_dir(destination)
        .and_then(|entries| entries.map(|entry| entry.map(|entry| entry.file_name())).collect())
        .map_err(|error| PreserveModulesFailure {
            error,
            preserved: PreservedModules::Merged {
                backup: backup.to_path_buf(),
                moved_entries: Vec::new(),
            },
        })?;
    let destination_entries: HashSet<OsString> = destination_entries;
    let source_entries = fs::read_dir(backup).map_err(|error| PreserveModulesFailure {
        error,
        preserved: PreservedModules::Merged {
            backup: backup.to_path_buf(),
            moved_entries: Vec::new(),
        },
    })?;
    let mut moved_entries = Vec::new();

    for entry in source_entries {
        let entry = entry.map_err(|error| PreserveModulesFailure {
            error,
            preserved: PreservedModules::Merged {
                backup: backup.to_path_buf(),
                moved_entries: moved_entries.clone(),
            },
        })?;
        let name = entry.file_name();
        if destination_entries.contains(&name) {
            continue;
        }
        fs::rename(entry.path(), destination.join(&name)).map_err(|error| {
            PreserveModulesFailure {
                error,
                preserved: PreservedModules::Merged {
                    backup: backup.to_path_buf(),
                    moved_entries: moved_entries.clone(),
                },
            }
        })?;
        moved_entries.push(name);
    }
    Ok(PreservedModules::Merged { backup: backup.to_path_buf(), moved_entries })
}

fn is_modules_dir_collision(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AlreadyExists
            | io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::PermissionDenied,
    )
}

/// Combined post-failure cleanup for steps 4 and 5: restore the
/// preserved `node_modules/` if it was moved, then rimraf the
/// staging directory — but only if the restore actually ran.
/// Leaving the staging tree and any merge backup on disk after a
/// failed restore retains every remaining copy of the preserved data.
fn finalize_stage_cleanup_after_failure(
    preserved_modules: &PreservedModules,
    stage: &Path,
    stage_modules: &Path,
    target_modules: &Path,
) {
    let restored = restore_preserved_node_modules(preserved_modules, stage_modules, target_modules);
    if restored {
        let _ = fs::remove_dir_all(stage);
    } else {
        leak_stage(stage, stage_modules, preserved_modules);
    }
}

/// Best-effort restoration of the preserved `node_modules/` directory
/// onto its original path. Returns `true` when there was nothing to
/// restore or the restoration succeeded; returns `false` when the
/// caller must not clean up the staging directory (it contains the
/// user's only copy of the data).
fn restore_preserved_node_modules(
    preserved_modules: &PreservedModules,
    stage_modules: &Path,
    target_modules: &Path,
) -> bool {
    let result = match preserved_modules {
        PreservedModules::None => return true,
        PreservedModules::Directory => fs::rename(stage_modules, target_modules),
        PreservedModules::Merged { backup, moved_entries } => {
            let restored_backup = moved_entries
                .iter()
                .try_for_each(|entry| fs::rename(stage_modules.join(entry), backup.join(entry)));
            restored_backup.and_then(|()| fs::rename(backup, target_modules))
        }
    };
    if let Err(error) = result {
        tracing::warn!(
            target: "pacquet::import_indexed_dir",
            ?stage_modules,
            ?target_modules,
            %error,
            "failed to restore preserved node_modules/ after a partial stage-and-swap",
        );
        false
    } else {
        true
    }
}

fn discard_replaced_modules(preserved_modules: &PreservedModules) {
    if let PreservedModules::Merged { backup, .. } = preserved_modules
        && let Err(error) = fs::remove_dir_all(backup)
    {
        tracing::warn!(
            target: "pacquet::import_indexed_dir",
            ?backup,
            %error,
            "failed to remove replaced node_modules/ entries after a successful stage-and-swap",
        );
    }
}

/// Emit a warning that the staging directory is being left in place
/// because removing it would destroy preserved data. Used by both
/// post-failure cleanup paths.
fn leak_stage(stage: &Path, stage_modules: &Path, preserved_modules: &PreservedModules) {
    let modules_backup = match preserved_modules {
        PreservedModules::Merged { backup, .. } => Some(backup),
        PreservedModules::None | PreservedModules::Directory => None,
    };
    tracing::warn!(
        target: "pacquet::import_indexed_dir",
        ?stage,
        ?stage_modules,
        ?modules_backup,
        "temporary paths left in place after a partial stage-and-swap because the preserved \
         node_modules/ could not be restored to its original location; recover manually from \
         the reported paths",
    );
}

/// Remove a non-directory dirent at `path`.
///
/// On Unix `fs::remove_file` unlinks any non-directory inode (regular
/// file, symlink-to-anywhere, fifo, socket). On Windows it rejects
/// directory symlinks and junctions — the OS treats those as
/// directory-shaped and they have to go through `remove_dir` instead.
/// Detect that case by resolving the link's target; if the target is
/// a directory (or the link is dangling but reports as a symlink),
/// route through `remove_dir`.
fn remove_non_dir_dirent(path: &Path, file_type: fs::FileType) -> io::Result<()> {
    #[cfg(windows)]
    if file_type.is_symlink() {
        // Resolved metadata follows the symlink: if the link points
        // at a directory (or is a junction, which Rust models as a
        // symlink whose target is a directory), `remove_dir` is the
        // correct call. Fall through to `remove_file` for dangling
        // links or symlinks-to-file.
        if matches!(fs::metadata(path), Ok(meta) if meta.is_dir()) {
            return fs::remove_dir(path);
        }
    }
    let _ = file_type;
    fs::remove_file(path)
}

/// Build a sibling path next to `target` that is unique within the
/// process. Mirrors pnpm's `fastPathTemp(newDir)` from the `path-temp`
/// package — same parent (so the final rename stays on one filesystem)
/// and a base name derived from the target so leaked staging dirs are
/// recognisable. Uniqueness across concurrent calls comes from PID +
/// wall-clock nanos + an atomic counter; we only need a process-local
/// guarantee because rayon worker threads are the only concurrent
/// callers.
fn pick_stage_path(target: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("dir");
    let pid = std::process::id();
    let ctr = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    parent.join(format!("{name}_pacquet-stage_{pid}_{nanos}_{ctr}"))
}

#[cfg(test)]
mod tests;
