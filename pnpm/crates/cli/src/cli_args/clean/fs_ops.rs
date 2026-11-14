use miette::{Context, IntoDiagnostic};
use pnpm_fs::{
    relative_path, remove_dirent, resolve_path_with_symlinks, symlink_metadata_with_retry,
    to_native_separators,
};
use std::{
    fs::Metadata,
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(super) fn resolve_configured_lockfile_dir(
    real_root_dir: &Path,
    raw_lockfile_dir: &Path,
) -> Option<PathBuf> {
    let candidate = resolve_inside(real_root_dir, raw_lockfile_dir)?;
    let real_lockfile_dir =
        resolve_path_with_symlinks(&candidate).and_then(dunce::canonicalize).ok()?;
    (real_lockfile_dir.starts_with(real_root_dir) && real_lockfile_dir != real_root_dir).then_some(
        real_lockfile_dir,
    )
}

pub(super) fn resolve_project_modules_dir(
    real_root_dir: &Path,
    project_dir: &Path,
    modules_leaf: &Path,
) -> Option<PathBuf> {
    let real_project_dir = dunce::canonicalize(project_dir).ok()?;
    let resolved = resolve_inside(&real_project_dir, &project_dir.join(modules_leaf))?;
    (resolved.starts_with(real_root_dir) && resolved != real_root_dir).then_some(resolved)
}

/// `path` with its parent resolved through symlinks, when it names an entry
/// strictly inside canonical `real_root`.
pub(super) fn resolve_inside(real_root: &Path, path: &Path) -> Option<PathBuf> {
    let path = to_native_separators(path);
    let file_name = path.file_name()?;
    if file_name
        .to_string_lossy()
        .trim_end_matches(['.', ' '])
        .is_empty()
    {
        return None;
    }
    let resolved = resolve_path_with_symlinks(path.parent()?)
        .and_then(dunce::canonicalize)
        .ok()?
        .join(file_name);
    (resolved.starts_with(real_root) && resolved != real_root).then_some(resolved)
}

pub(super) fn inspect_existing_path(path: &Path) -> miette::Result<Option<Metadata>> {
    match symlink_metadata_with_retry(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            Ok(None)
        }
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("inspecting {}", path.display())),
    }
}

pub(super) fn remove_path(path: &Path) -> miette::Result<()> {
    remove_dirent(path)
        .or_else(|error| if error.kind() == ErrorKind::NotFound { Ok(()) } else { Err(error) })
        .into_diagnostic()
        .wrap_err_with(|| format!("removing {}", path.display()))
}

pub(super) fn print_removing(base: &Path, path: &Path) {
    let relative = relative_path(base, path);
    let owned: PathBuf =
        if relative.as_os_str().is_empty() { PathBuf::from(".") } else { relative };
    pnpm_diagnostics::println!("Removing {}", owned.display());
}
