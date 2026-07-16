use crate::{
    LinkFileError, import_into_fresh_target,
    remove_quarantine::remove_quarantine_from_native_binaries,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::PackageImportMethod;
use pacquet_reporter::Reporter;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
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
/// shape (no force and no nested-modules preservation). The hoisted linker
/// always forces import and preserves nested modules.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImportIndexedDirOpts {
    /// When `true`, re-import even when `dir_path` already exists,
    /// overwriting the existing contents. Clone-or-copy may also replace an
    /// existing target that is not already private and owner-writable.
    pub force: bool,
    /// When `true` (only meaningful with `force`), preserve
    /// `dir_path/node_modules/` across the re-import so nested
    /// dependencies survive the rebuild. Required by the hoisted
    /// linker, whose orphan-removal and insert passes are
    /// interleaved across the package tree — a nested `node_modules/`
    /// installed by a sibling pass must not be clobbered when the
    /// parent package is re-imported.
    pub keep_modules_dir: bool,
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
    #[display("failed to place completion marker {from:?} at {to:?}: {error}")]
    PlaceMarker {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to make private package projection writable at {path:?}: {error}")]
    MakeWritable {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Materialize an indexed package's files into `dir_path`, the way
/// pnpm v11's `importIndexedDir` does at
/// `fs/indexed-pkg-importer/src/importIndexedDir.ts`. The same function
/// services both node-linkers; behavior at the destination is
/// controlled by [`ImportIndexedDirOpts`].
///
/// Files in `cas_paths` are materialized using `import_method`'s preference
/// order. [`PackageImportMethod::CloneOrCopy`] creates a private owner-writable
/// projection. The first use of each import tier during an install emits a
/// `pnpm:package-import-method` event through `logged_methods`.
pub fn import_indexed_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    opts: ImportIndexedDirOpts,
) -> Result<(), ImportIndexedDirError> {
    let private_writable = import_method == PackageImportMethod::CloneOrCopy;
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
    let force = opts.force
        || (private_writable
            && existing_kind.as_ref().is_some_and(|file_type| {
                !file_type.is_dir() || !package_is_private_and_writable(dir_path, cas_paths)
            }));

    // Drop the macOS quarantine xattr from the package's native binaries after
    // a populating import, matching pnpm's `removeQuarantineFromNativeBinaries`.
    // The marker-present short-circuit (and the non-directory dirent left as-is)
    // import nothing, so they skip the sweep, keeping warm installs free of the
    // per-install `xattr` cost — exactly pnpm's `!pkgExistsAtTargetDir` gate.
    let unquarantine = || remove_quarantine_from_native_binaries(dir_path, cas_paths);
    match (existing_kind, force) {
        (None, _) => populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
            .inspect(|()| unquarantine()),
        // Short-circuit only when the completion marker is present
        // (pnpm's `pkgExistsAtTargetDir`),
        // not on mere directory existence. A marker-less directory is a
        // partial import; repair it by re-running the non-destructive
        // `populate_dir`. Ported from pnpm/pnpm#12204 (cbfeeef328).
        (Some(file_type), false) if file_type.is_dir() => {
            if marker_present(dir_path, cas_paths) {
                Ok(())
            } else {
                populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
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
            populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
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

/// Fresh-target path: make the parent dir set, then run the parallel
/// `import_into_fresh_target` over `cas_paths`. Mirrors pnpm v11's
/// `tryImportIndexedDir`: collect the unique relative parent dirs,
/// sort shortest-first, mkdir each sequentially, then dispatch the
/// file imports in parallel. Sorting by length means the recursive
/// mkdir for a deeper dir always finds its ancestor already on disk,
/// so each call costs one `mkdirat` instead of walking up.
fn populate_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
) -> Result<(), ImportIndexedDirError> {
    let private_writable = import_method == PackageImportMethod::CloneOrCopy;
    // The package root itself: pnpm's `importIndexedDir` mkdirs
    // `newDir` before calling `tryImportIndexedDir`, so do that here
    // too. Files at the package root (e.g. `package.json`) need this
    // even when `rel_dirs` is empty.
    fs::create_dir_all(dir_path).map_err(|error| ImportIndexedDirError::CreateDir {
        dirname: dir_path.to_path_buf(),
        error,
    })?;
    if private_writable {
        make_owner_writable(dir_path)?;
    }

    for rel in package_directories(cas_paths) {
        let abs = dir_path.join(rel);
        fs::create_dir_all(&abs)
            .map_err(|error| ImportIndexedDirError::CreateDir { dirname: abs, error })?;
        if private_writable {
            make_owner_writable(&dir_path.join(rel))?;
        }
    }

    // Link every other file first, then place the marker last, so an
    // interrupted import leaves a directory the next install recognises
    // as incomplete (pnpm's `tryImportIndexedDir`).
    let marker = marker_file(cas_paths);
    cas_paths
        .par_iter()
        .filter(|(cleaned_entry, _)| Some(cleaned_entry.as_str()) != marker)
        .try_for_each(|(cleaned_entry, store_path)| {
            // No pre-flight stat: `import_into_fresh_target` tolerates an
            // existing target (the repair branch re-links over a partial
            // directory), so the stat would be pure overhead — ~170k saved
            // syscalls on the alotta-files fixture.
            let target = dir_path.join(cleaned_entry);
            import_into_fresh_target::<Reporter>(
                logged_methods,
                import_method,
                store_path,
                &target,
            )
            .map_err(ImportIndexedDirError::LinkFile)?;
            if private_writable { make_owner_writable(&target) } else { Ok(()) }
        })?;

    if let Some(marker) = marker {
        import_marker_atomic::<Reporter>(
            logged_methods,
            import_method,
            &cas_paths[marker],
            &dir_path.join(marker),
        )?;
    }
    Ok(())
}

fn package_directories(cas_paths: &HashMap<String, PathBuf>) -> Vec<&str> {
    let mut directories = HashSet::new();
    for entry in cas_paths.keys() {
        let mut parent = Path::new(entry).parent();
        while let Some(path) = parent {
            if let Some(relative) = path.to_str()
                && !relative.is_empty()
            {
                directories.insert(relative);
            }
            parent = path.parent();
        }
    }
    let mut ordered: Vec<_> = directories.into_iter().collect();
    ordered.sort_by_key(|path| path.len());
    ordered
}

/// The completion-marker filename, mirroring pnpm's `pickFileFromFilesMap`:
/// `package.json` when present, else a fallback file for old store entries
/// indexed before the synthetic manifest. pnpm picks the first inserted
/// key; `cas_paths` is unordered, so we pick the lexicographically
/// smallest one instead — deterministic, which is all the gate and the
/// write need. `None` only for an empty map.
fn marker_file(cas_paths: &HashMap<String, PathBuf>) -> Option<&str> {
    const PACKAGE_JSON: &str = "package.json";
    if cas_paths.contains_key(PACKAGE_JSON) {
        return Some(PACKAGE_JSON);
    }
    cas_paths.keys().map(String::as_str).min()
}

/// Whether `dir_path` holds the completion marker. An empty map has no
/// marker, so it counts as present — there is nothing to import.
fn marker_present(dir_path: &Path, cas_paths: &HashMap<String, PathBuf>) -> bool {
    match marker_file(cas_paths) {
        Some(marker) => dir_path.join(marker).exists(),
        None => true,
    }
}

fn package_is_private_and_writable(dir_path: &Path, cas_paths: &HashMap<String, PathBuf>) -> bool {
    let Ok(root_metadata) = fs::symlink_metadata(dir_path) else {
        return false;
    };
    if !root_metadata.file_type().is_dir() || !is_owner_usable_directory(&root_metadata) {
        return false;
    }
    for relative in package_directories(cas_paths) {
        let Ok(metadata) = fs::symlink_metadata(dir_path.join(relative)) else {
            return false;
        };
        if !metadata.file_type().is_dir() || !is_owner_usable_directory(&metadata) {
            return false;
        }
    }
    cas_paths.iter().all(|(entry, _)| {
        let target = dir_path.join(entry);
        let Ok(metadata) = fs::symlink_metadata(&target) else {
            return false;
        };
        metadata.file_type().is_file()
            && is_owner_writable(&metadata)
            && has_single_hard_link(&target, &metadata)
    })
}

/// Place the marker atomically (pnpm's `importFileAtomic`): link it into a
/// private temp sibling, then rename onto `marker_path` so it is never
/// observed half-written. pacquet picks its import tier at runtime, so it
/// always stages rather than predicting whether the import will copy. A
/// concurrent importer that placed the marker first surfaces as
/// `AlreadyExists` or is replaced atomically; the content is
/// content-addressed either way, so we just drop our temp.
fn import_marker_atomic<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    store_path: &Path,
    marker_path: &Path,
) -> Result<(), ImportIndexedDirError> {
    let temp = pick_stage_path(marker_path);
    import_into_fresh_target::<Reporter>(logged_methods, import_method, store_path, &temp)
        .map_err(ImportIndexedDirError::LinkFile)?;
    if import_method == PackageImportMethod::CloneOrCopy
        && let Err(error) = make_owner_writable(&temp)
    {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    match fs::rename(&temp, marker_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(ImportIndexedDirError::PlaceMarker {
                from: temp,
                to: marker_path.to_path_buf(),
                error,
            })
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
    let target_modules = dir_path.join("node_modules");
    let stage_modules = stage.join("node_modules");

    // 1. Populate the staging directory with the new contents. On
    //    failure, the staging directory is the only thing on disk we
    //    own — a blanket rimraf is safe.
    if let Err(error) = populate_dir::<Reporter>(logged_methods, import_method, &stage, cas_paths) {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    if import_method == PackageImportMethod::CloneOrCopy {
        let preserved_modules = keep_modules_dir.then(|| dir_path.join("node_modules"));
        if let Err(error) = make_existing_tree_removable(dir_path, preserved_modules.as_deref()) {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
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

    if import_method == PackageImportMethod::CloneOrCopy
        && nm_kind.as_ref().is_some_and(fs::FileType::is_dir)
        && let Err(error) = make_directory_removable(&target_modules)
    {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    // 3. Preserve `node_modules/` if it's a real directory. A package may
    //    contain bundled dependencies in the staged tree, so merge only names
    //    that the package did not supply. Track the move so steps 4 and 5 can
    //    rescue it on failure.
    let nm_moved = match nm_kind {
        Some(file_type) if file_type.is_dir() => {
            if stage_modules.exists() {
                if let Err(error) = merge_modules_dirs(&target_modules, &stage_modules) {
                    finalize_stage_cleanup_after_failure(
                        true,
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
            } else if let Err(error) = fs::rename(&target_modules, &stage_modules) {
                let _ = fs::remove_dir_all(&stage);
                return Err(ImportIndexedDirError::PreserveModulesDir {
                    from: target_modules,
                    to: stage_modules,
                    error,
                });
            }
            true
        }
        Some(_) | None => false,
    };

    // 4. Remove the old contents. If this fails after step 3, the
    //    staged copy of `node_modules/` is the user's only copy —
    //    try to move it back into place before bailing, and leak
    //    the staging directory if the move can't run.
    if let Err(error) = fs::remove_dir_all(dir_path) {
        finalize_stage_cleanup_after_failure(nm_moved, &stage, &stage_modules, &target_modules);
        return Err(ImportIndexedDirError::RemoveExisting { path: dir_path.to_path_buf(), error });
    }

    // 5. Move the staged tree into place. There's a brief window
    //    between `remove_dir_all` and `rename` where `dir_path` does
    //    not exist on disk — acceptable given pacquet runs one
    //    install per process and the hoisted linker holds the install
    //    graph's coarse lock. If the rename fails, recreate
    //    `dir_path` so the rescued `node_modules/` has somewhere to
    //    land.
    if let Err(error) = fs::rename(&stage, dir_path) {
        // `create_dir_all` is the gate: without `dir_path`, the rescue
        // rename has no destination. Treat its failure as "rescue
        // can't run" and leak the staging directory below.
        let rescue_target_ready = !nm_moved || fs::create_dir_all(dir_path).is_ok();
        if rescue_target_ready {
            finalize_stage_cleanup_after_failure(nm_moved, &stage, &stage_modules, &target_modules);
        } else {
            leak_stage(&stage, &stage_modules);
        }
        return Err(ImportIndexedDirError::Swap { from: stage, to: dir_path.to_path_buf(), error });
    }
    Ok(())
}

fn make_existing_tree_removable(
    dir_path: &Path,
    preserved_dir: Option<&Path>,
) -> Result<(), ImportIndexedDirError> {
    let mut pending = vec![dir_path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        make_directory_removable(&directory)?;
        let entries = fs::read_dir(&directory).map_err(|error| {
            ImportIndexedDirError::InspectTarget { path: directory.clone(), error }
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| ImportIndexedDirError::InspectTarget {
                path: directory.clone(),
                error,
            })?;
            let path = entry.path();
            if preserved_dir == Some(path.as_path()) {
                continue;
            }
            let file_type = entry.file_type().map_err(|error| {
                ImportIndexedDirError::InspectTarget { path: path.clone(), error }
            })?;
            if file_type.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn make_directory_removable(directory: &Path) -> Result<(), ImportIndexedDirError> {
    match make_owner_writable(directory) {
        Ok(()) => Ok(()),
        #[cfg(unix)]
        Err(ImportIndexedDirError::MakeWritable { error, .. })
            if error.kind() == io::ErrorKind::PermissionDenied =>
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};

            // Opening a mode-000 directory cannot produce the descriptor used
            // by the no-follow helper. This tree is a stale private projection
            // beneath the install root, so make only the directory traversable
            // before removing it.
            let metadata = fs::symlink_metadata(directory).map_err(|error| {
                ImportIndexedDirError::InspectTarget { path: directory.to_path_buf(), error }
            })?;
            if !metadata.file_type().is_dir() {
                return Err(ImportIndexedDirError::MakeWritable {
                    path: directory.to_path_buf(),
                    error,
                });
            }
            fs::set_permissions(directory, fs::Permissions::from_mode(metadata.mode() | 0o700))
                .map_err(|error| ImportIndexedDirError::MakeWritable {
                    path: directory.to_path_buf(),
                    error,
                })
        }
        Err(error) => Err(error),
    }
}

fn make_owner_writable(target: &Path) -> Result<(), ImportIndexedDirError> {
    pacquet_fs::file_mode::make_path_owner_writable(target)
        .map_err(|error| ImportIndexedDirError::MakeWritable { path: target.to_path_buf(), error })
}

#[cfg(unix)]
fn has_single_hard_link(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() == 1
}

#[cfg(windows)]
fn has_single_hard_link(path: &Path, _metadata: &fs::Metadata) -> bool {
    pacquet_fs::file_mode::hard_link_count(path).is_ok_and(|count| count == 1)
}

#[cfg(unix)]
fn is_owner_writable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o200 != 0
}

#[cfg(unix)]
fn is_owner_usable_directory(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o700 == 0o700
}

#[cfg(windows)]
fn is_owner_writable(metadata: &fs::Metadata) -> bool {
    !metadata.permissions().readonly()
}

#[cfg(windows)]
fn is_owner_usable_directory(metadata: &fs::Metadata) -> bool {
    is_owner_writable(metadata)
}

/// Combined post-failure cleanup for steps 4 and 5: restore the
/// preserved `node_modules/` if it was moved, then rimraf the
/// staging directory — but only if the restore actually ran.
/// Leaving the staging directory on disk after a failed restore is
/// deliberate: it contains the user's only copy of the preserved
/// `node_modules/`, and silently destroying it would compound the
/// install failure with data loss. The emit warning gives an
/// operator the exact path to recover from.
fn finalize_stage_cleanup_after_failure(
    nm_moved: bool,
    stage: &Path,
    stage_modules: &Path,
    target_modules: &Path,
) {
    let restored = restore_preserved_node_modules(nm_moved, stage_modules, target_modules);
    if restored {
        let _ = fs::remove_dir_all(stage);
    } else {
        leak_stage(stage, stage_modules);
    }
}

/// Best-effort restoration of the preserved `node_modules/` directory
/// onto its original path. Returns `true` when there was nothing to
/// restore or the restoration succeeded; returns `false` when the
/// caller must not clean up the staging directory (it contains the
/// user's only copy of the data).
fn restore_preserved_node_modules(
    nm_moved: bool,
    stage_modules: &Path,
    target_modules: &Path,
) -> bool {
    if !nm_moved {
        return true;
    }
    match fs::rename(stage_modules, target_modules) {
        Ok(()) => true,
        Err(rename_error) => match merge_modules_dirs(stage_modules, target_modules) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::import_indexed_dir",
                    ?stage_modules,
                    ?target_modules,
                    %rename_error,
                    %error,
                    "failed to restore preserved node_modules/ after a partial stage-and-swap",
                );
                false
            }
        },
    }
}

fn merge_modules_dirs(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let dest_files = fs::read_dir(dest)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<HashSet<_>>>()?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if !dest_files.contains(&entry.file_name()) {
            fs::rename(entry.path(), dest.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Emit a warning that the staging directory is being left in place
/// because removing it would destroy preserved data. Used by both
/// post-failure cleanup paths.
fn leak_stage(stage: &Path, stage_modules: &Path) {
    tracing::warn!(
        target: "pacquet::import_indexed_dir",
        ?stage,
        ?stage_modules,
        "staging directory left in place after a partial stage-and-swap because the preserved \
         node_modules/ could not be restored to its original location; recover manually from \
         the staged copy",
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
