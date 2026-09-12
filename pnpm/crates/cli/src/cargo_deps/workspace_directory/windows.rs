use super::{
    IntoDiagnostic, ManagedDirectory, Path, PathBuf, Result, accept_existing_directory, fs, io,
};
use miette::WrapErr;

#[cfg(windows)]
pub(in super::super) fn ensure_workspace_directory_windows(
    root: PathBuf,
    components: &[&str],
) -> Result<ManagedDirectory> {
    let root_handle = open_pinned_windows_directory(&root)
        .into_diagnostic()
        .wrap_err_with(|| format!("open Cargo workspace directory {}", root.display()))?;
    let root_metadata = root_handle
        .metadata()
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect Cargo workspace directory {}", root.display()))?;
    if !root_metadata.is_dir() || is_windows_reparse_point(&root_metadata) {
        let root = root.display();
        return Err(miette::miette!(
            "managed Cargo workspace directory {} must be a real directory",
            root,
        ));
    }
    let mut handles = vec![root_handle];
    let mut path = root;
    for component in components {
        path.push(component);
        let handle = open_or_create_pinned_windows_directory(&path)?;
        let metadata = handle
            .metadata()
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()))?;
        if !metadata.is_dir() || is_windows_reparse_point(&metadata) {
            let path = path.display();
            return Err(miette::miette!(
                "managed Cargo directory {} must be a real directory",
                path,
            ));
        }
        handles.push(handle);
    }
    // Windows lacks the descriptor-relative operations used on Unix. Keeping
    // every component open without FILE_SHARE_DELETE prevents a checked parent
    // from being renamed or replaced while the path-based helpers run.
    Ok(ManagedDirectory { path, _pinned_components: handles })
}

#[cfg(windows)]
fn open_or_create_pinned_windows_directory(path: &Path) -> Result<fs::File> {
    loop {
        match open_pinned_windows_directory(path) {
            Ok(handle) => return Ok(handle),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(path)
                    .or_else(accept_existing_directory)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("create Cargo directory {}", path.display()))?;
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()));
            }
        }
    }
}

#[cfg(windows)]
fn open_pinned_windows_directory(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
