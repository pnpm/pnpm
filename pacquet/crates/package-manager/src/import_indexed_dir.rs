use crate::{LinkFileError, import_into_fresh_target};
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
    #[display(
        "the indexed file map already contains a node_modules/ entry at {path:?}, which would conflict with the directory being preserved"
    )]
    NodeModulesCollision { path: PathBuf },
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
}

/// Materialize an indexed package's files into `dir_path`, the way
/// pnpm v11's `importIndexedDir` does at
/// `fs/indexed-pkg-importer/src/importIndexedDir.ts`. The same function
/// services both node-linkers; behavior at the destination is
/// controlled by [`ImportIndexedDirOpts`]:
///
/// * **Default opts (isolated linker).** If `dir_path` already exists,
///   short-circuit; otherwise mkdir parents and link each file in
///   parallel via `import_into_fresh_target()`. Matches pnpm's
///   `importIndexedPackage` when called without `force`.
/// * **`opts.force` (hoisted linker).** Re-import even when `dir_path`
///   exists. The new contents are staged in a sibling directory so the
///   final rename stays on one filesystem, the old directory is
///   removed, and the staging directory is renamed into place. A
///   regular file or symlink occupying `dir_path` is unlinked first.
/// * **`opts.force` + `opts.keep_modules_dir` (hoisted linker).**
///   Before the swap, `dir_path/node_modules/` is moved into the
///   staging directory so nested deps survive the rebuild. On any
///   failure after the move, the staged copy is restored to
///   `dir_path/node_modules/` before the staging directory is
///   cleaned up — staging never holds the user's only copy of nested
///   deps. Required by the hoisted linker's interleaved orphan-removal
///   and insert passes.
///
/// Files in `cas_paths` are materialized by `import_into_fresh_target()`
/// using `import_method`'s preference order
/// (hardlink → reflink → copy, etc.), and the per-method
/// `pnpm:package-import-method` log is emitted via `logged_methods`
/// the first time each tier is used in this install. The pre-flight
/// `fs::metadata` short-circuit lives on `link_file()` for callers
/// without a freshness guarantee — `populate_dir` already gates on
/// the directory being fresh, so it skips that stat.
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

    match (existing_kind, opts.force) {
        // Fresh target — populate it. Both linkers take this path on
        // first install.
        (None, _) => populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths),
        // Existing directory with force=false — pnpm's pre-existence
        // short-circuit, but keyed on the *completion marker*, not the
        // mere existence of the directory (`pkgExistsAtTargetDir` in
        // `fs/indexed-pkg-importer/src/index.ts`, which checks for
        // `package.json`). A directory that exists but lacks the marker
        // is a partial result from an interrupted import — `populate_dir`
        // writes the marker last (see below), so its absence proves the
        // earlier import never finished. Repair it by re-running
        // `populate_dir`, which is non-destructive and EEXIST-tolerant:
        // it links only the files still missing and leaves a sibling
        // importer's work intact. This matches pnpm's `safeToSkip`
        // (global-virtual-store) branch, which likewise re-links into an
        // existing directory rather than destructively replacing it.
        // Ported from pnpm commit cbfeeef328 (pnpm/pnpm#12204).
        (Some(file_type), false) if file_type.is_dir() => {
            if marker_present(dir_path, cas_paths) {
                Ok(())
            } else {
                populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
            }
        }
        // Existing non-directory dirent with force=false. A stale symlink
        // or file occupying the slot is left as-is, matching the prior
        // short-circuit; the force=true path below is the only one that
        // clobbers a non-directory dirent.
        (Some(_), false) => Ok(()),
        // Existing non-directory dirent with force=true. The hoisted
        // linker call shape won't produce this in practice, but
        // refusing to clobber a stale symlink would wedge the install.
        (Some(file_type), true) if !file_type.is_dir() => {
            remove_non_dir_dirent(dir_path, file_type).map_err(|error| {
                ImportIndexedDirError::ClearNonDirEntry { path: dir_path.to_path_buf(), error }
            })?;
            populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
        }
        // Existing directory with force=true — stage and swap.
        (Some(_), true) => stage_and_swap::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            opts.keep_modules_dir,
        ),
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
        let abs = dir_path.join(rel);
        fs::create_dir_all(&abs)
            .map_err(|error| ImportIndexedDirError::CreateDir { dirname: abs, error })?;
    }

    // pnpm writes the completion marker (`package.json`) last, so a
    // crash mid-import leaves a directory the next install recognises as
    // incomplete (`tryImportIndexedDir` in
    // `fs/indexed-pkg-importer/src/importIndexedDir.ts`). Link every
    // other file first, in parallel, then place the marker.
    let marker = marker_file(cas_paths);
    cas_paths
        .par_iter()
        .filter(|(cleaned_entry, _)| Some(cleaned_entry.as_str()) != marker)
        .try_for_each(|(cleaned_entry, store_path)| {
            // Targets are guaranteed fresh: `populate_dir` only runs
            // against a freshly-mkdir'd `dir_path` (the caller in
            // `import_indexed_dir` gates on the directory not existing,
            // or staged it as a new sibling). Skipping the pre-flight
            // `fs::metadata` saves one `stat` syscall per file — ~170k
            // on the alotta-files fixture — without losing the
            // no-op-on-existing-target contract that `link_file` exposes
            // to other callers.
            import_into_fresh_target::<Reporter>(
                logged_methods,
                import_method,
                store_path,
                &dir_path.join(cleaned_entry),
            )
        })
        .map_err(ImportIndexedDirError::LinkFile)?;

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

/// Pick the completion-marker filename out of an indexed file map,
/// mirroring pnpm's `pickFileFromFilesMap`
/// (`fs/indexed-pkg-importer/src/index.ts`): prefer `package.json` (the
/// worker synthesises one for every package), and otherwise fall back to
/// a single file so old store entries indexed before the synthetic
/// `package.json` still get a marker. pnpm's fallback is the map's
/// first-by-insertion-order key; pacquet's `cas_paths` is a `HashMap`
/// with no stable order, so we take the lexicographically smallest key —
/// deterministic, and good enough since the gate
/// ([`marker_present`]) and the marker write
/// ([`populate_dir`]) both consult this same function. Returns `None`
/// only for an empty map, where there is nothing to import or gate on.
fn marker_file(cas_paths: &HashMap<String, PathBuf>) -> Option<&str> {
    const PACKAGE_JSON: &str = "package.json";
    if cas_paths.contains_key(PACKAGE_JSON) {
        return Some(PACKAGE_JSON);
    }
    cas_paths.keys().map(String::as_str).min()
}

/// Whether `dir_path` already holds the completion marker for this file
/// map. An empty map has no marker to check, so it counts as present —
/// there is nothing to import, matching the prior "directory exists ⇒
/// done" short-circuit for that degenerate case.
fn marker_present(dir_path: &Path, cas_paths: &HashMap<String, PathBuf>) -> bool {
    match marker_file(cas_paths) {
        Some(marker) => dir_path.join(marker).exists(),
        None => true,
    }
}

/// Import the completion marker so it appears atomically: link it into a
/// private temp sibling, then rename it onto the marker path. pnpm uses
/// its `importFileAtomic` for `package.json` (a temp-file + rename on the
/// copy path; the raw syscall on the already-atomic hardlink/reflink
/// paths). pacquet's `Auto` / `CloneOrCopy` methods pick their tier at
/// runtime, so rather than predict whether this import will copy, always
/// stage-and-rename — one extra rename per package, and the marker can
/// never be observed half-written. A concurrent importer that placed the
/// marker first surfaces as `AlreadyExists` (Windows) or is replaced
/// atomically (POSIX `rename`); either way the content is
/// content-addressed, so the result is equivalent and we drop our temp.
fn import_marker_atomic<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    store_path: &Path,
    marker_path: &Path,
) -> Result<(), ImportIndexedDirError> {
    let temp = pick_stage_path(marker_path);
    import_into_fresh_target::<Reporter>(logged_methods, import_method, store_path, &temp)
        .map_err(ImportIndexedDirError::LinkFile)?;
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

    // 3. Preserve `node_modules/` if it's a real directory. Track the
    //    move so steps 4 and 5 can rescue it on failure.
    //
    //    Indexed file maps for npm tarballs never contain
    //    `node_modules/` entries (npm and pnpm strip them at pack
    //    time), so a pre-existing `<stage>/node_modules/` would be
    //    pathological; surface it as an error rather than silently
    //    merging. Upstream's `moveOrMergeModulesDirs` performs a real
    //    merge for this case, but the hoisted-linker call site does
    //    not exercise it in practice.
    let nm_moved = match nm_kind {
        Some(file_type) if file_type.is_dir() => {
            if stage_modules.exists() {
                let _ = fs::remove_dir_all(&stage);
                return Err(ImportIndexedDirError::NodeModulesCollision { path: stage_modules });
            }
            if let Err(error) = fs::rename(&target_modules, &stage_modules) {
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
        Err(error) => {
            tracing::warn!(
                target: "pacquet::import_indexed_dir",
                ?stage_modules,
                ?target_modules,
                %error,
                "failed to restore preserved node_modules/ after a partial stage-and-swap",
            );
            false
        }
    }
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
