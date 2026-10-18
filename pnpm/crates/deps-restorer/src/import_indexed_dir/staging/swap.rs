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
/// The occupied target is moved aside by a rename rather than removed,
/// so the path is never absent while it is being replaced: an importer
/// that finds the target absent can rename its own stage in without the
/// swap lock, and that must mean nothing was there yet. The displaced
/// tree is removed once the staged one is in place.
pub(super) fn replace_target(
    paths: &StagePaths,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserved_modules: &PreservedModules,
) -> Result<(), ImportIndexedDirError> {
    let displaced = match move_target_aside(dir_path) {
        Ok(displaced) => displaced,
        // A tree that cannot be renamed, as Windows refuses while a
        // process runs from it, is removed in place instead.
        Err(_) => match remove_target_for_swap(
            dir_path,
            cas_paths,
            !preserved_modules.has_moved_data(),
            paths.keep_modules_dir,
        ) {
            Ok(true) => {
                finish_concurrent_import(paths, preserved_modules);
                return Ok(());
            }
            Ok(false) => None,
            Err(error) => {
                paths.cleanup_after_failure(preserved_modules);
                return Err(ImportIndexedDirError::RemoveExisting {
                    path: dir_path.to_path_buf(),
                    error,
                });
            }
        },
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
    if let Some(displaced) = &displaced {
        discard_displaced_target(displaced);
    }
    Ok(())
}

/// Rename the occupied target to a sibling path. `None` when nothing is
/// at `dir_path`.
fn move_target_aside(dir_path: &Path) -> Result<Option<PathBuf>, io::Error> {
    let aside = pick_stage_path(dir_path);
    match pnpm_fs::rename_with_retry(dir_path, &aside) {
        Ok(()) => Ok(Some(aside)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
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

fn discard_displaced_target(displaced: &Path) {
    if let Err(error) = pnpm_fs::remove_dirent(displaced) {
        tracing::warn!(
            target: "pacquet::import_indexed_dir",
            ?displaced,
            %error,
            "failed to remove the replaced package directory after a successful stage-and-swap",
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
