use super::{
    super::{ImportIndexedDirError, PreservedModules, all_files_match},
    StagePaths, discard_replaced_modules,
};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DIRECTORY_SWAP_RETRY_BUDGET: Duration = Duration::from_secs(30);
const DIRECTORY_SWAP_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(super) fn replace_target(
    paths: &StagePaths,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserved_modules: &PreservedModules,
) -> Result<(), ImportIndexedDirError> {
    match remove_target_for_swap(dir_path, cas_paths, !preserved_modules.has_moved_data()) {
        Ok(true) => {
            finish_concurrent_import(paths, preserved_modules);
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            paths.cleanup_after_failure(preserved_modules);
            return Err(ImportIndexedDirError::RemoveExisting {
                path: dir_path.to_path_buf(),
                error,
            });
        }
    }
    if let Err(error) = swap_staged_dir(paths, dir_path, cas_paths, preserved_modules) {
        paths.rescue_or_leak(preserved_modules, dir_path);
        return Err(ImportIndexedDirError::Swap {
            from: paths.stage.clone(),
            to: dir_path.to_path_buf(),
            error,
        });
    }
    Ok(())
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
        if remove_target_for_swap(dir_path, cas_paths, !preserved_modules.has_moved_data())? {
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
        Err(_) if !preserved_modules.has_moved_data() && all_files_match(dir_path, cas_paths) => {
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
) -> Result<bool, io::Error> {
    let retry_deadline = Instant::now() + DIRECTORY_SWAP_RETRY_BUDGET;
    loop {
        match pnpm_fs::remove_dirent(dir_path) {
            Ok(()) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error)
                if is_directory_swap_collision(&error)
                    && accept_complete
                    && all_files_match(dir_path, cas_paths) =>
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
