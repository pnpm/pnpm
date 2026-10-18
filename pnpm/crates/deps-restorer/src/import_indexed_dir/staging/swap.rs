use super::{
    super::{ImportIndexedDirError, PreservedModules},
    StagePaths, discard_replaced_modules, pick_stage_path,
};
use crate::import_indexed_dir::placement::exact::all_imported_entries_match_exactly;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DIRECTORY_SWAP_RETRY_BUDGET: Duration = Duration::from_secs(30);
const DIRECTORY_SWAP_RETRY_DELAY: Duration = Duration::from_millis(10);

/// Put the staged tree at `dir_path` in place of what is there.
///
/// Where the platform exchanges two dirents atomically, the target path
/// is never absent and never names a partial tree. Elsewhere the occupied
/// target is moved aside by a rename and the stage renamed in, which
/// leaves the path absent between the two renames. An importer that found
/// the target absent renames its own stage in without the swap lock, and
/// that rename fails against a tree that landed in the meantime, so the
/// vacancy is open only to an importer that checked the target during it,
/// or that stalled with its stage ready until it. Such an importer's tree
/// is equivalent and is replaced or kept by the swap that follows.
///
/// Returns the path the displaced tree now sits at, for the caller to
/// remove once the swap lock is released: the removal touches only that
/// private sibling path, and a swap waiting for the lock need not wait
/// for it.
pub(super) fn replace_target(
    paths: &StagePaths,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserved_modules: &PreservedModules,
) -> Result<Option<PathBuf>, ImportIndexedDirError> {
    if pnpm_fs::exchange_dirents(&paths.stage, dir_path).is_ok() {
        discard_replaced_modules(preserved_modules);
        return Ok(Some(paths.stage.clone()));
    }
    let displaced = match displace_target(paths, dir_path, cas_paths, preserved_modules) {
        Ok(Displaced::Equivalent) => {
            finish_concurrent_import(paths, preserved_modules);
            return Ok(None);
        }
        Ok(Displaced::Aside(aside)) => Some(aside),
        Ok(Displaced::Gone) => None,
        Err(error) => {
            paths.cleanup_after_failure(preserved_modules);
            return Err(ImportIndexedDirError::RemoveExisting {
                path: dir_path.to_path_buf(),
                error,
            });
        }
    };
    if let Err(error) = swap_staged_dir(paths, dir_path, cas_paths, preserved_modules) {
        if let Some(displaced) = &displaced {
            restore_displaced_target(displaced, dir_path);
        }
        paths.rescue_or_leak(preserved_modules, dir_path);
        return Err(ImportIndexedDirError::Swap {
            from: paths.stage.clone(),
            to: dir_path.to_path_buf(),
            error,
        });
    }
    Ok(displaced)
}

/// Where the occupied target went to make room for the staged tree.
enum Displaced {
    /// Renamed to this sibling path.
    Aside(PathBuf),
    /// Removed, or never there.
    Gone,
    /// Kept: a concurrent import completed an equivalent tree that could
    /// not be removed from under it.
    Equivalent,
}

fn displace_target(
    paths: &StagePaths,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserved_modules: &PreservedModules,
) -> Result<Displaced, io::Error> {
    let aside = pick_stage_path(dir_path);
    match pnpm_fs::rename_with_retry(dir_path, &aside) {
        Ok(()) => return Ok(Displaced::Aside(aside)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Displaced::Gone),
        // Windows refuses to rename a directory a process runs from; such
        // a tree is removed in place instead.
        Err(_) => {}
    }
    let accepted = remove_target_for_swap(
        dir_path,
        cas_paths,
        !preserved_modules.has_moved_data(),
        paths.keep_modules_dir,
    )?;
    Ok(if accepted { Displaced::Equivalent } else { Displaced::Gone })
}

/// Put the displaced target back after a failed swap, if the path is
/// free. A displaced tree that stays behind keeps the user's data; the
/// rescue of the preserved `node_modules/` reports the paths involved.
fn restore_displaced_target(displaced: &Path, dir_path: &Path) {
    if let Err(error) = pnpm_fs::rename_with_retry(displaced, dir_path) {
        tracing::warn!(
            target: "pacquet::import_indexed_dir",
            ?displaced,
            ?dir_path,
            %error,
            "failed to move the replaced package directory back after a failed stage-and-swap",
        );
    }
}

fn swap_staged_dir(
    paths: &StagePaths,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserved_modules: &PreservedModules,
) -> Result<(), io::Error> {
    let retry_deadline = Instant::now() + DIRECTORY_SWAP_RETRY_BUDGET;
    loop {
        if try_rename(paths, dir_path, cas_paths, preserved_modules)? {
            return Ok(());
        }
        if Instant::now() >= retry_deadline {
            return Err(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                "concurrent package import did not settle",
            ));
        }
        if remove_target_for_swap(
            dir_path,
            cas_paths,
            !preserved_modules.has_moved_data(),
            paths.keep_modules_dir,
        )? {
            finish_concurrent_import(paths, preserved_modules);
            return Ok(());
        }
        std::thread::sleep(DIRECTORY_SWAP_RETRY_DELAY);
    }
}

fn try_rename(
    paths: &StagePaths,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserved_modules: &PreservedModules,
) -> Result<bool, io::Error> {
    match pnpm_fs::rename_with_retry(&paths.stage, dir_path) {
        Ok(()) => {
            discard_replaced_modules(preserved_modules);
            Ok(true)
        }
        Err(error) if !is_rename_target_collision(&error) => Err(error),
        Err(_)
            if !preserved_modules.has_moved_data()
                && all_imported_entries_match_exactly(
                    dir_path,
                    cas_paths,
                    paths.keep_modules_dir,
                ) =>
        {
            finish_concurrent_import(paths, preserved_modules);
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

fn remove_target_for_swap(
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    accept_complete: bool,
    preserve_node_modules: bool,
) -> Result<bool, io::Error> {
    let retry_deadline = Instant::now() + DIRECTORY_SWAP_RETRY_BUDGET;
    loop {
        match pnpm_fs::remove_dirent(dir_path) {
            Ok(()) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error)
                if is_directory_swap_collision(&error)
                    && accept_complete
                    && all_imported_entries_match_exactly(
                        dir_path,
                        cas_paths,
                        preserve_node_modules,
                    ) =>
            {
                return Ok(true);
            }
            Err(error) if is_directory_swap_collision(&error) => {
                if Instant::now() >= retry_deadline {
                    return Err(error);
                }
                std::thread::sleep(DIRECTORY_SWAP_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_directory_swap_collision(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::DirectoryNotEmpty | io::ErrorKind::AlreadyExists)
}

fn is_rename_target_collision(error: &io::Error) -> bool {
    is_directory_swap_collision(error)
        || cfg!(windows) && error.kind() == io::ErrorKind::PermissionDenied
}

fn finish_concurrent_import(paths: &StagePaths, preserved_modules: &PreservedModules) {
    discard_replaced_modules(preserved_modules);
    let _ = std::fs::remove_dir_all(&paths.stage);
}
