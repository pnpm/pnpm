use std::{io, path::Path};

/// Bit mask to filter executable bits (`--x--x--x`).
pub const EXEC_MASK: u32 = 0b001_001_001;

/// All can read and execute, but only owner can write (`rwxr-xr-x`).
pub const EXEC_MODE: u32 = 0b111_101_101;

/// Whether a file mode has *any* executable bit set (`u+x`, `g+x`, or
/// `o+x`). Matches pnpm's `modeIsExecutable` and is therefore the rule
/// pacquet must follow when deciding whether a CAFS blob gets the
/// `-exec` suffix or has its on-disk mode flipped executable.
#[must_use]
pub fn is_executable(mode: u32) -> bool {
    mode & EXEC_MASK != 0
}

/// Whether a CAS file path encodes "executable" via the `-exec` suffix
/// pnpm's CAFS layout uses (see `pacquet_store_dir::StoreDir::cas_file_path`).
/// Reading the suffix is cheaper than a `stat` and is the only reliable
/// signal once a blob has been copied out of the store, where the on-disk
/// mode may have lost its exec bit on a copy / reflink fallback.
#[must_use]
pub fn cas_path_is_executable(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with("-exec"))
}

/// Re-add executable bits to `target` when the CAS source path carries the
/// `-exec` suffix. The CAS encodes executability purely in that suffix (see
/// [`cas_path_is_executable`]), so it is the source of truth: a `copy` or
/// `reflink` that materializes a freshly-created `0o644` target — `fs::copy`
/// on overlayfs, Linux `FICLONE` reflink always — would otherwise leave a
/// native binary non-executable. Non-executable entries (no `-exec` suffix)
/// are left untouched, so the mode is never widened and no `set_permissions`
/// syscall is paid on the non-exec majority. On Windows this is a no-op
/// because POSIX permission bits do not apply.
pub fn restore_exec_bit_from_cas_suffix(cas_path: &Path, target: &Path) -> io::Result<()> {
    #[cfg(unix)]
    if cas_path_is_executable(cas_path) {
        // Open once and chmod through the fd. A path-based
        // `metadata()` + `set_permissions()` pair leaves a TOCTOU window where
        // a concurrent writer could swap `target` between the two calls and
        // redirect the chmod onto an unintended inode; binding both to one
        // opened file closes it. Defense-in-depth on the install hot path.
        //
        // Retry the open under fd-table exhaustion like every other open on
        // the parallel import path: a transient `EMFILE`/`ENFILE` from a
        // sibling rayon worker must not fail the install.
        let file = crate::ensure_file::retry_on_fd_pressure(|| std::fs::File::open(target))?;
        make_file_executable(&file)?;
    }
    #[cfg(not(unix))]
    let _ = (cas_path, target);
    Ok(())
}

/// Add the executable bits (`u+x g+x o+x`) to `file`, a no-op on Windows.
///
/// Skips the `set_permissions` syscall (and the ctime bump it would cause) when
/// every exec bit is already set, so re-asserting executability on a file that
/// already has it costs only the stat.
#[cfg_attr(windows, allow(unused))]
pub fn make_file_executable(file: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    return add_mode_bits(file, EXEC_MASK);

    #[cfg(windows)]
    return Ok(());
}

/// Add owner-write permission without following a symlink at the final path.
/// Directories also receive owner-read and owner-execute so the owner can
/// enumerate, traverse, and modify their entries.
pub fn make_path_owner_writable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    let file = crate::ensure_file::retry_on_fd_pressure(|| {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path)
    })?;
    #[cfg(windows)]
    let file = open_windows_path(
        path,
        windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES
            | windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES,
    )?;
    #[cfg(windows)]
    {
        let metadata = file.metadata()?;
        if metadata.file_type().is_symlink() {
            return Err(symlink_path_error(path));
        }
        make_windows_file_writable(&file, metadata.permissions())
    }
    #[cfg(unix)]
    {
        let bits = if file.metadata()?.is_dir() { 0o700 } else { 0o200 };
        add_mode_bits(&file, bits)
    }
}

/// Return the number of hard links to a path without following a final symlink.
pub fn hard_link_count(path: &Path) -> io::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(std::fs::symlink_metadata(path)?.nlink())
    }

    #[cfg(windows)]
    {
        use std::{mem, os::windows::io::AsRawHandle};
        use windows_sys::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{
                FILE_READ_ATTRIBUTES, FILE_STANDARD_INFO, FileStandardInfo,
                GetFileInformationByHandleEx,
            },
        };

        let file = open_windows_path(path, FILE_READ_ATTRIBUTES)?;
        if file.metadata()?.file_type().is_symlink() {
            return Err(symlink_path_error(path));
        }
        let mut info = FILE_STANDARD_INFO::default();
        // SAFETY: `file` owns a valid handle and `info` is a correctly sized,
        // writable `FILE_STANDARD_INFO` buffer for the duration of the call.
        let result = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle() as HANDLE,
                FileStandardInfo,
                (&mut info as *mut FILE_STANDARD_INFO).cast(),
                mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(u64::from(info.NumberOfLinks))
    }
}

#[cfg(windows)]
fn open_windows_path(path: &Path, access_mode: u32) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    crate::ensure_file::retry_on_fd_pressure(|| {
        std::fs::OpenOptions::new()
            .access_mode(access_mode)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    })
}

#[cfg(windows)]
#[expect(
    clippy::permissions_set_readonly_false,
    reason = "this Windows-only helper clears FILE_ATTRIBUTE_READONLY"
)]
fn make_windows_file_writable(
    file: &std::fs::File,
    mut permissions: std::fs::Permissions,
) -> io::Result<()> {
    if permissions.readonly() {
        permissions.set_readonly(false);
        file.set_permissions(permissions)?;
    }
    Ok(())
}

#[cfg(windows)]
fn symlink_path_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("refusing to operate on symlink at {}", path.display()),
    )
}

#[cfg(unix)]
fn add_mode_bits(file: &std::fs::File, bits: u32) -> io::Result<()> {
    use std::{
        fs::Permissions,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };
    let mode = file.metadata()?.mode();
    if mode & bits == bits {
        return Ok(());
    }
    file.set_permissions(Permissions::from_mode(mode | bits))
}

#[cfg(test)]
mod tests;
