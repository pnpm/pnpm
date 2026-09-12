use super::{
    ImportIndexedDirError, Placement, PreserveModulesFailure, PreservedModules,
    existing_dirent_kind, file_matches_store_entry, populate_dir,
};
use crate::import_into_fresh_target;
use pnpm_config::PackageImportMethod;
use pnpm_fs::{Host, rename_even_across_devices};
use pnpm_reporter::Reporter;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Place a file atomically (pnpm's `importFileAtomic`): link it into a
/// private temp sibling, then rename onto `target` so it is never
/// observed half-written, and so a stale copy is replaced in one step
/// under a reader. pacquet picks its import tier at runtime, so it
/// always stages rather than predicting whether the import will copy.
/// A failed rename is accepted only when the destination now matches the
/// store entry, which means a concurrent importer won the race with the
/// same content.
pub(super) fn import_atomic<Reporter: self::Reporter>(
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
pub(super) fn stage_and_swap<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    keep_modules_dir: bool,
) -> Result<(), ImportIndexedDirError> {
    let paths = StagePaths::new(dir_path);

    // 1. Populate the staging directory with the new contents. On
    //    failure, the staging directory is the only thing on disk we
    //    own — a blanket rimraf is safe.
    if let Err(error) = populate_dir::<Reporter>(
        logged_methods,
        import_method,
        &paths.stage,
        cas_paths,
        Placement::Fresh,
    ) {
        let _ = fs::remove_dir_all(&paths.stage);
        return Err(error);
    }

    // 2. Move the existing `node_modules/` aside so nested deps survive
    //    the swap.
    let preserved_modules = paths.preserve_modules(keep_modules_dir)?;

    // 3. Remove the old contents. If this fails after step 2, the
    //    staged tree and any merge backup hold the preserved data. Try
    //    to move it back into place before bailing, and retain those
    //    temporary paths if restoration can't run.
    if let Err(error) = pnpm_fs::remove_dir_all_with_retry(dir_path) {
        paths.cleanup_after_failure(&preserved_modules);
        return Err(ImportIndexedDirError::RemoveExisting { path: dir_path.to_path_buf(), error });
    }

    // 4. Move the staged tree into place. There's a brief window
    //    between `remove_dir_all` and `rename` where `dir_path` does
    //    not exist on disk — acceptable for a slot only this install
    //    can reach; a shared slot never enters this function.
    if let Err(error) = pnpm_fs::rename_with_retry(&paths.stage, dir_path) {
        paths.rescue_or_leak(&preserved_modules, dir_path);
        return Err(ImportIndexedDirError::Swap {
            from: paths.stage,
            to: dir_path.to_path_buf(),
            error,
        });
    }
    discard_replaced_modules(&preserved_modules);
    Ok(())
}
/// The temporary paths a staged swap works through, and the recovery
/// steps that put the target back together when one of its phases
/// fails.
pub(super) struct StagePaths {
    stage: PathBuf,
    modules_backup: PathBuf,
    target_modules: PathBuf,
    stage_modules: PathBuf,
}
impl StagePaths {
    fn new(dir_path: &Path) -> Self {
        let stage = pick_stage_path(dir_path);
        StagePaths {
            modules_backup: pick_stage_path(dir_path),
            target_modules: dir_path.join("node_modules"),
            stage_modules: stage.join("node_modules"),
            stage,
        }
    }

    /// Preserve the target's `node_modules/` if it is a real directory.
    /// A package may ship bundled dependencies, so non-conflicting
    /// top-level entries are merged when the staged import has its own
    /// `node_modules/`. The staged package wins conflicts, matching
    /// pnpm's `moveOrMergeModulesDirs`.
    fn preserve_modules(
        &self,
        keep_modules_dir: bool,
    ) -> Result<PreservedModules, ImportIndexedDirError> {
        let Some(file_type) = self.existing_modules_kind(keep_modules_dir)? else {
            return Ok(PreservedModules::None);
        };
        if !file_type.is_dir() {
            return Ok(PreservedModules::None);
        }
        match preserve_modules_dir(&self.target_modules, &self.stage_modules, &self.modules_backup)
        {
            Ok(preserved) => Ok(preserved),
            Err(PreserveModulesFailure { error, preserved }) => {
                self.cleanup_after_failure(&preserved);
                Err(ImportIndexedDirError::PreserveModulesDir {
                    from: self.target_modules.clone(),
                    to: self.stage_modules.clone(),
                    error,
                })
            }
        }
    }

    /// Only `NotFound` is benign here — `PermissionDenied` and other
    /// transient I/O failures must surface, otherwise the user's nested
    /// deps get silently clobbered when the directory is removed.
    fn existing_modules_kind(
        &self,
        keep_modules_dir: bool,
    ) -> Result<Option<fs::FileType>, ImportIndexedDirError> {
        if !keep_modules_dir {
            return Ok(None);
        }
        existing_dirent_kind(&self.target_modules).inspect_err(|_| {
            let _ = fs::remove_dir_all(&self.stage);
        })
    }

    fn cleanup_after_failure(&self, preserved: &PreservedModules) {
        finalize_stage_cleanup_after_failure(
            preserved,
            &self.stage,
            &self.stage_modules,
            &self.target_modules,
        );
    }

    /// `create_dir_all` is the gate: without `dir_path`, the rescue
    /// rename has no destination. Treat its failure as "rescue can't
    /// run" and leak the staging directory instead.
    fn rescue_or_leak(&self, preserved: &PreservedModules, dir_path: &Path) {
        if !preserved.has_moved_data() || fs::create_dir_all(dir_path).is_ok() {
            self.cleanup_after_failure(preserved);
        } else {
            leak_stage(&self.stage, &self.stage_modules, preserved);
        }
    }
}
pub(super) fn preserve_modules_dir(
    source: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<PreservedModules, PreserveModulesFailure> {
    match rename_even_across_devices::<Host>(source, destination) {
        Ok(()) => return Ok(PreservedModules::Directory),
        Err(error) if is_modules_dir_collision(&error) => {}
        Err(error) => {
            return Err(PreserveModulesFailure { error, preserved: PreservedModules::None });
        }
    }

    rename_even_across_devices::<Host>(source, backup)
        .map_err(|error| PreserveModulesFailure { error, preserved: PreservedModules::None })?;

    merge_preserved_modules(destination, backup)
}
pub(super) fn merge_preserved_modules(
    destination: &Path,
    backup: &Path,
) -> Result<PreservedModules, PreserveModulesFailure> {
    let destination_entries = preserved_destination_entries(destination, backup)?;
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
        rename_even_across_devices::<Host>(&entry.path(), &destination.join(&name)).map_err(
            |error| PreserveModulesFailure {
                error,
                preserved: PreservedModules::Merged {
                    backup: backup.to_path_buf(),
                    moved_entries: moved_entries.clone(),
                },
            },
        )?;
        moved_entries.push(name);
    }
    Ok(PreservedModules::Merged { backup: backup.to_path_buf(), moved_entries })
}
pub(super) fn preserved_destination_entries(
    destination: &Path,
    backup: &Path,
) -> Result<HashSet<OsString>, PreserveModulesFailure> {
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
    Ok(destination_entries)
}
pub(super) fn is_modules_dir_collision(error: &io::Error) -> bool {
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
pub(super) fn finalize_stage_cleanup_after_failure(
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
pub(super) fn restore_preserved_node_modules(
    preserved_modules: &PreservedModules,
    stage_modules: &Path,
    target_modules: &Path,
) -> bool {
    let result = match preserved_modules {
        PreservedModules::None => return true,
        PreservedModules::Directory => {
            rename_even_across_devices::<Host>(stage_modules, target_modules)
        }
        PreservedModules::Merged { backup, moved_entries } => {
            let restored_backup = moved_entries.iter().try_for_each(|entry| {
                rename_even_across_devices::<Host>(&stage_modules.join(entry), &backup.join(entry))
            });
            restored_backup
                .and_then(|()| rename_even_across_devices::<Host>(backup, target_modules))
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
pub(super) fn discard_replaced_modules(preserved_modules: &PreservedModules) {
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
pub(super) fn leak_stage(stage: &Path, stage_modules: &Path, preserved_modules: &PreservedModules) {
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
/// Build a sibling path next to `target` that is unique within the
/// process. Mirrors pnpm's `fastPathTemp(newDir)` from the `path-temp`
/// package — same parent (so the final rename stays on one filesystem)
/// and a base name derived from the target so leaked staging dirs are
/// recognisable. Uniqueness across concurrent calls comes from PID +
/// wall-clock nanos + an atomic counter; we only need a process-local
/// guarantee because rayon worker threads are the only concurrent
/// callers.
pub(super) fn pick_stage_path(target: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("dir");
    let pid = std::process::id();
    let ctr = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    parent.join(format!("{name}_pacquet-stage_{pid}_{nanos}_{ctr}"))
}
