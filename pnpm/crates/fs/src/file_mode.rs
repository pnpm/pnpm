use std::{
    io,
    path::{Path, PathBuf},
};

/// Bit mask to filter executable bits (`--x--x--x`).
pub const EXEC_MASK: u32 = 0b001_001_001;

/// All can read and execute, but only owner can write (`rwxr-xr-x`).
pub const EXEC_MODE: u32 = 0b111_101_101;

/// All can read and write (`rw-rw-rw-`), the create mode for
/// non-executable entries.
pub const BASE_FILE_MODE: u32 = 0b110_110_110;

/// The mode a fresh store write gives an entry of `executable`'s class
/// under `umask`. The store creates every file this way
/// (`StoreDir::write_cas_file`), so materializing a store file at this
/// mode is what a store write would have produced, whatever umask
/// populated the store (pnpm/pnpm#3807).
#[must_use]
pub fn store_entry_mode(executable: bool, umask: u32) -> u32 {
    (if executable { EXEC_MODE } else { BASE_FILE_MODE }) & !umask & 0o777
}

/// The process's current umask, read once: the CLI never changes it, and
/// the import hot path would otherwise pay two `umask(2)` syscalls per
/// file.
#[cfg(unix)]
#[must_use]
pub fn current_umask() -> u32 {
    static UMASK: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| {
        // SAFETY: `umask` is always safe to call. Reading the mask requires
        // setting it, so both calls are made back to back with nothing in
        // between but the other `umask` call, and this runs once per process
        // behind a `LazyLock` initializer. A file another thread creates in
        // that window gets the unmasked creation mode, which carries no
        // executable bit and does not match any desired mode, so the import
        // tiers copy it instead of linking it.
        let mask = unsafe { libc::umask(0) };
        // SAFETY: Restores the mask the call above read, making the read
        // invisible to every other thread.
        unsafe { libc::umask(mask) };
        widen_mode(mask)
    });
    *UMASK
}

/// `libc::umask` speaks `mode_t`, a `u16` on macOS and a `u32` on Linux.
/// `Into` is the one widening both platforms' clippy accepts: a cast is
/// `cast_lossless` on macOS and `u32::from` is `useless_conversion` on
/// Linux.
#[cfg(unix)]
fn widen_mode(mode: impl Into<u32>) -> u32 {
    mode.into()
}

/// [`current_umask`] on platforms without mode bits: nothing to mask.
#[cfg(not(unix))]
#[must_use]
pub fn current_umask() -> u32 {
    0
}

/// Whether a file mode has *any* executable bit set (`u+x`, `g+x`, or
/// `o+x`). Matches pnpm's `modeIsExecutable` and is therefore the rule
/// pacquet must follow when deciding whether a CAFS blob gets the
/// `-exec` suffix or has its on-disk mode flipped executable.
#[must_use]
pub fn is_executable(mode: u32) -> bool {
    mode & EXEC_MASK != 0
}

/// Whether a CAS file path encodes "executable" via the `-exec` suffix
/// pnpm's CAFS layout uses (see `pnpm_store_dir::StoreDir::cas_file_path`).
/// Reading the suffix is cheaper than a `stat` and is the only reliable
/// signal once a blob has been copied out of the store, where the on-disk
/// mode may have lost its exec bit on a copy / reflink fallback.
#[must_use]
pub fn cas_path_is_executable(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("-exec"))
}

/// Open `path` for permission changes,
/// refusing to traverse a final symlink.
///
/// Callers require a regular file. A symlink there is corruption or a squatter, and
/// following it would hand the referent an execute bit it never had:
/// `O_NOFOLLOW` answers `ELOOP` instead, and the caller reports it.
#[cfg(unix)]
fn open_without_following(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
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
        let file = crate::ensure_file::retry_on_fd_pressure(|| open_without_following(target))?;
        make_file_executable(&file)?;
    }
    #[cfg(not(unix))]
    let _ = (cas_path, target);
    Ok(())
}

/// Set Unix permission bits without following a final symlink. No-op on Windows.
pub fn set_path_permissions(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};
        let file = crate::ensure_file::retry_on_fd_pressure(|| open_without_following(path))?;
        if file.metadata()?.permissions().mode() & 0o7777 != mode {
            file.set_permissions(Permissions::from_mode(mode))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

/// Add the executable bits (`u+x g+x o+x`) to `file`, a no-op on Windows.
///
/// Skips the `set_permissions` syscall (and the ctime bump it would cause) when
/// every exec bit is already set, so re-asserting executability on a file that
/// already has it costs only the stat.
#[cfg_attr(windows, allow(unused, reason = "POSIX executable bits do not apply on Windows"))]
pub fn make_file_executable(file: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    return {
        use std::{
            fs::Permissions,
            os::unix::fs::{MetadataExt, PermissionsExt},
        };
        let mode = file.metadata()?.mode();
        if mode & EXEC_MASK == EXEC_MASK {
            return Ok(());
        }
        file.set_permissions(Permissions::from_mode(mode | EXEC_MASK))
    };

    #[cfg(windows)]
    return Ok(());
}

/// Permission bits a new file takes from its parent directory.
///
/// Read and write bits come from the directory, so a group-writable store
/// stays group-writable for the next user. Execute bits are copied only when
/// `executable` is set, and only for classes that can search the directory.
/// Setuid, setgid, and sticky are not copied onto a file. Owner read and
/// write are always set so the creating process can finish the write.
#[must_use]
pub fn inherited_file_mode(parent_mode: u32, executable: bool) -> u32 {
    let mut mode = parent_mode & 0o666;
    if executable {
        if parent_mode & 0o100 != 0 {
            mode |= 0o100;
        }
        if parent_mode & 0o010 != 0 {
            mode |= 0o010;
        }
        if parent_mode & 0o001 != 0 {
            mode |= 0o001;
        }
    }
    mode | 0o600
}

/// Open-mode ceiling for a new file, and the bits to add after create.
///
/// The open mode is a ceiling. A default ACL can grant only bits that are
/// present in it, and umask can only remove bits. [`grant_mode_bits`] adds
/// back bits umask stripped, without clearing bits the create already set.
/// An explicit private mode (no group or other bits, such as `0o600`) is
/// used as the ceiling and is not widened.
#[cfg(unix)]
#[must_use]
pub fn unix_creation_mode(parent: &Path, requested: Option<u32>) -> UnixCreationMode {
    if requested.is_some_and(|mode| mode & 0o077 == 0) {
        return UnixCreationMode { open_mode: requested, grant_mode: None };
    }
    let executable = requested.is_some_and(is_executable);
    match std::fs::metadata(parent) {
        Ok(meta) => {
            use std::os::unix::fs::PermissionsExt;
            let wanted = inherited_file_mode(meta.permissions().mode(), executable);
            UnixCreationMode { open_mode: Some(wanted), grant_mode: Some(wanted) }
        }
        Err(_) => UnixCreationMode { open_mode: requested, grant_mode: None },
    }
}

/// Open ceiling and the post-create grant for [`unix_creation_mode`].
#[cfg(unix)]
#[derive(Clone, Copy)]
pub struct UnixCreationMode {
    pub open_mode: Option<u32>,
    pub grant_mode: Option<u32>,
}

/// OR `wanted` onto `file`. Bits already present are kept, so a default ACL
/// wider than the directory mode survives. `EPERM`, `EACCES`, and `EROFS`
/// are ignored: the file is already usable by its creator, and a store entry
/// this process does not own must not fail the install.
#[cfg(unix)]
pub fn grant_mode_bits(file: &std::fs::File, wanted: u32) -> io::Result<()> {
    use std::{fs::Permissions, os::unix::fs::PermissionsExt};
    let current = match file.metadata() {
        Ok(meta) => meta.permissions().mode() & 0o777,
        Err(error) if is_unchangeable(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let merged = current | (wanted & 0o777);
    if merged == current {
        return Ok(());
    }
    match file.set_permissions(Permissions::from_mode(merged)) {
        Err(error) if is_unchangeable(&error) => Ok(()),
        other => other,
    }
}

/// OR the file mode inherited from `parent` onto `file`.
///
/// No-op when `parent` cannot be stated. See [`grant_mode_bits`] for which
/// failures are ignored.
#[cfg(unix)]
pub fn grant_inherited_mode(
    file: &std::fs::File,
    parent: &Path,
    executable: bool,
) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let wanted = match std::fs::metadata(parent) {
        Ok(meta) => inherited_file_mode(meta.permissions().mode(), executable),
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    grant_mode_bits(file, wanted)
}

/// Closest existing directory at `dir` or above it.
///
/// A relative path whose parents are all missing resolves to `.` when the
/// working directory exists. Returns `None` when nothing on the path is a
/// directory.
#[must_use]
pub fn nearest_existing_ancestor(dir: &Path) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        if current.as_os_str().is_empty() {
            let dot = PathBuf::from(".");
            return dot.is_dir().then_some(dot);
        }
        if current.is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// After a missing directory tree is created, OR `template`'s group-write
/// and setgid bits onto each new directory, stopping before `template`.
///
/// Directories that already existed are not passed in. `EPERM`, `EACCES`,
/// and `EROFS` are ignored. The root directory is never changed.
#[cfg(unix)]
pub fn grant_inherited_dir_mode(dir: &Path, template: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let extra = match std::fs::metadata(template) {
        Ok(meta) => meta.permissions().mode() & (0o020 | 0o2000),
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if extra == 0 {
        return Ok(());
    }
    let mut current = dir.to_path_buf();
    while current != template {
        // Never chmod `/` when `template` is not a lexical prefix of `dir`.
        if current.as_os_str().is_empty() || current.parent().is_none() {
            break;
        }
        match std::fs::metadata(&current) {
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o7777;
                let merged = mode | extra;
                if merged != mode {
                    if let Err(error) =
                        std::fs::set_permissions(&current, std::fs::Permissions::from_mode(merged))
                    {
                        if !is_unchangeable(&error) {
                            return Err(error);
                        }
                    }
                }
            }
            Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !current.pop() {
            break;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_unchangeable(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem)
}

#[cfg(test)]
mod tests;
