#[cfg(windows)]
pub(super) use windows::ensure_workspace_directory_windows;

#[cfg(any(unix, windows))]
use super::fs;
use super::{IntoDiagnostic, Path, PathBuf, Result, io};
#[cfg(any(unix, windows))]
use miette::WrapErr;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
static MANAGED_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// As open as the umask allows, which is what `std::fs::create_dir` asks
/// for.
#[cfg(unix)]
const DIRECTORY_MODE: pnpm_ffi_fs::FileMode = pnpm_ffi_fs::FileMode::from_mode(0o777);

/// Owner-only, so a staged file is unreadable to anyone else even where
/// the umask would have allowed more.
#[cfg(unix)]
const PRIVATE_FILE_MODE: pnpm_ffi_fs::FileMode = pnpm_ffi_fs::FileMode::from_mode(0o600);

pub(super) struct ManagedDirectory {
    pub(super) path: PathBuf,
    #[cfg(unix)]
    pub(super) handle: fs::File,
    #[cfg(windows)]
    pub(super) _pinned_components: Vec<fs::File>,
}

pub(super) fn ensure_workspace_directory(
    root_dir: &Path,
    components: &[&str],
) -> Result<ManagedDirectory> {
    #[cfg(unix)]
    {
        ensure_workspace_directory_unix(root_dir.to_path_buf(), components)
    }
    #[cfg(target_os = "wasi")]
    {
        let _ = (root_dir, components);
        wasi::unsupported().into_diagnostic()
    }
    #[cfg(windows)]
    {
        let root = fs::canonicalize(root_dir)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("resolve Cargo workspace directory {}", root_dir.display())
            })?;
        ensure_workspace_directory_windows(root, components)
    }
}

#[cfg(unix)]
fn ensure_workspace_directory_unix(root: PathBuf, components: &[&str]) -> Result<ManagedDirectory> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY);
    let mut handle = options
        .open(&root)
        .into_diagnostic()
        .wrap_err_with(|| format!("open Cargo workspace directory {}", root.display()))?;
    let mut path = root;
    for component in components {
        path.push(component);
        handle = open_or_create_directory_at(&handle, component, &path)?;
    }
    Ok(ManagedDirectory { path, handle })
}

/// Open `component` under `parent`, creating it if it is not there yet.
/// The loop retries the open after a create because a concurrent
/// install may replace the entry in between.
#[cfg(unix)]
fn open_or_create_directory_at(
    parent: &fs::File,
    component: &str,
    path: &Path,
) -> Result<fs::File> {
    let component = std::ffi::OsStr::new(component);
    loop {
        match pnpm_ffi_fs::open_directory_at(parent, component) {
            Ok(handle) => return Ok(handle),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                pnpm_ffi_fs::create_directory_at(parent, component, DIRECTORY_MODE)
                    .or_else(accept_existing_directory)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("create Cargo directory {}", path.display()))?;
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotADirectory
                    || error.raw_os_error() == Some(libc::ELOOP) =>
            {
                let path = path.display();
                return Err(miette::miette!(
                    "managed Cargo directory {} must be a real directory",
                    path,
                ));
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()));
            }
        }
    }
}

#[cfg(any(unix, windows))]
fn accept_existing_directory(error: io::Error) -> io::Result<()> {
    match error.kind() {
        // Lost the race with a concurrent install; its directory is as good as ours.
        io::ErrorKind::AlreadyExists => Ok(()),
        _ => Err(error),
    }
}

#[cfg(unix)]
pub(super) fn read_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(String, Option<u32>)> {
    use std::os::unix::fs::PermissionsExt as _;

    let file = pnpm_ffi_fs::open_file_at(&directory.handle, std::ffi::OsStr::new(name))?;
    let mode = file.metadata()?.permissions().mode();
    let contents = io::read_to_string(file)?;
    Ok((contents, Some(mode)))
}

#[cfg(windows)]
pub(super) fn read_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(String, Option<u32>)> {
    fs::read_to_string(directory.path.join(name)).map(|contents| (contents, None))
}

#[cfg(unix)]
pub(super) fn write_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
    bytes: &[u8],
    mode: Option<u32>,
) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let (temporary, mut file) = create_workspace_temporary(directory, name)?;
    let result = (|| {
        file.write_all(bytes)?;
        if let Some(mode) = mode {
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        pnpm_fs::sync_file(&file)?;
        pnpm_ffi_fs::rename_at(&directory.handle, &temporary, std::ffi::OsStr::new(name))
    })();
    drop(file);
    if result.is_err() {
        let _ = pnpm_ffi_fs::unlink_at(&directory.handle, &temporary);
    }
    result
}

#[cfg(windows)]
pub(super) fn write_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
    bytes: &[u8],
    _mode: Option<u32>,
) -> io::Result<()> {
    pnpm_fs::write_atomic(&directory.path.join(name), bytes)
}

#[cfg(unix)]
pub(super) fn force_workspace_symlink(
    directory: &ManagedDirectory,
    target: &Path,
    name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    let wanted = pnpm_fs::relative_path(&directory.path, target);
    let entry = std::ffi::OsStr::new(name);
    let mut warning = None;
    loop {
        match pnpm_ffi_fs::symlink_at(&wanted, &directory.handle, entry) {
            Ok(()) => return Ok(pnpm_fs::ForceSymlinkOutcome { reused: false, warning }),
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
            Err(_) => {}
        }
        match pnpm_ffi_fs::read_link_at(&directory.handle, entry) {
            Ok(existing) if existing == wanted => {
                return Ok(pnpm_fs::ForceSymlinkOutcome { reused: true, warning });
            }
            // A symlink pointing somewhere else is ours to replace.
            Ok(_) => unlink_at(directory, entry)?,
            // `InvalidInput` from reading the link means the entry is not a
            // symlink at all, so it is moved aside rather than removed.
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                warning = Some(move_occupant_aside(directory, entry, name)?);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
}

/// Remove the entry, tolerating a concurrent install having removed it
/// first.
#[cfg(unix)]
fn unlink_at(directory: &ManagedDirectory, name: &std::ffi::OsStr) -> io::Result<()> {
    match pnpm_ffi_fs::unlink_at(&directory.handle, name) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Rename the non-symlink occupying the wanted name out of the way,
/// returning the warning the caller reports for it.
#[cfg(unix)]
fn move_occupant_aside(
    directory: &ManagedDirectory,
    entry: &std::ffi::OsStr,
    name: &str,
) -> io::Result<String> {
    let ignored_name = format!(
        ".ignored_{name}-{}-{}",
        pnpm_fs::process_id(),
        MANAGED_TEMP_ID.fetch_add(1, Ordering::Relaxed),
    );
    pnpm_ffi_fs::rename_at(&directory.handle, entry, std::ffi::OsStr::new(&ignored_name))?;
    Ok(format!(
        "Symlink wanted name was occupied by directory or file. Old entity moved: {:?}{}{} => {ignored_name}",
        directory.path,
        std::path::MAIN_SEPARATOR,
        name,
    ))
}

#[cfg(windows)]
pub(super) fn force_workspace_symlink(
    directory: &ManagedDirectory,
    target: &Path,
    name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    pnpm_fs::force_symlink_dir(target, &directory.path.join(name))
}

#[cfg(unix)]
fn create_workspace_temporary(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(std::ffi::OsString, fs::File)> {
    loop {
        let temporary = std::ffi::OsString::from(format!(
            ".{name}.pnpm-{}-{}",
            pnpm_fs::process_id(),
            MANAGED_TEMP_ID.fetch_add(1, Ordering::Relaxed),
        ));
        match pnpm_ffi_fs::create_file_at(&directory.handle, &temporary, PRIVATE_FILE_MODE) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
mod windows;

#[cfg(target_os = "wasi")]
mod wasi;
#[cfg(target_os = "wasi")]
pub(super) use wasi::{force_workspace_symlink, read_workspace_file, write_workspace_file};
