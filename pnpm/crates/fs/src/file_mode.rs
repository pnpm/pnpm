#[cfg(any(unix, target_os = "wasi"))]
pub use directory::grant_inherited_dir_mode;
pub use directory::{
    create_dir_all_inheriting_mode, inherited_dir_bits, nearest_existing_ancestor,
};

mod directory;
#[cfg(unix)]
use directory::is_unchangeable;

use pnpm_ffi_fs::FileMode;
use std::{io, path::Path};

/// Bit mask to filter executable bits (`--x--x--x`).
pub const EXEC_MASK: u32 = 0b001_001_001;

/// All can read and execute, but only owner can write (`rwxr-xr-x`).
pub const EXEC_MODE: FileMode = FileMode::from_mode(0b111_101_101);

/// All can read and write (`rw-rw-rw-`), the create mode for
/// non-executable entries.
pub const BASE_FILE_MODE: FileMode = FileMode::from_mode(0b110_110_110);

/// The mode a fresh store write gives an entry of `executable`'s class
/// under `umask`. The store creates every file this way
/// (`StoreDir::write_cas_file`), so materializing a store file at this
/// mode is what a store write would have produced, whatever umask
/// populated the store (pnpm/pnpm#3807).
#[must_use]
pub fn store_entry_mode(executable: bool, umask: u32) -> FileMode {
    let base = if executable { EXEC_MODE } else { BASE_FILE_MODE };
    FileMode::from_mode(base.mode() & !umask & 0o777)
}

/// Whether a store inode of `mode` can be linked where [`store_entry_mode`]
/// gives `desired`.
///
/// Group- and other-write may be missing, and group-write may be extra:
/// a store write takes those bits from its shard directory
/// ([`inherited_file_mode`]), not from the umask, and the shared inode
/// already grants them through the store. Every other bit must match, so a
/// store populated under a wider umask is still not linked (pnpm/pnpm#3807).
#[must_use]
pub fn store_inode_mode_is_linkable(mode: FileMode, desired: FileMode) -> bool {
    const GROUP_OTHER_WRITE: u32 = 0o022;
    let (mode, desired) = (mode.mode(), desired.mode());
    (mode ^ desired) & 0o777 & !GROUP_OTHER_WRITE == 0 && mode & !desired & 0o002 == 0
}

/// The process's current umask, read once: the CLI never changes it, and
/// the import hot path would otherwise pay a syscall per file.
#[cfg(unix)]
#[must_use]
pub fn current_umask() -> u32 {
    static UMASK: std::sync::LazyLock<u32> =
        std::sync::LazyLock::new(pnpm_ffi_process::current_umask);
    *UMASK
}

/// Cache the host process mask for WASI permission calculations.
#[cfg(target_os = "wasi")]
#[must_use]
pub fn current_umask() -> u32 {
    static UMASK: std::sync::LazyLock<u32> =
        std::sync::LazyLock::new(crate::wasi_fs::current_umask);
    *UMASK
}

/// Platforms without permission bits have no mode bits to mask.
#[cfg(not(any(unix, target_os = "wasi")))]
#[must_use]
pub fn current_umask() -> u32 {
    0
}

/// Whether a file mode has *any* executable bit set (`u+x`, `g+x`, or
/// `o+x`). Matches pnpm's `modeIsExecutable`.
#[must_use]
pub fn is_executable(mode: FileMode) -> bool {
    mode.mode() & EXEC_MASK != 0
}

/// The permission bits pnpm records for a store entry built from
/// `metadata`: the permission bits alone on Unix, and a fixed `0o644`
/// where the host has none, which is why a mode difference is not
/// compared on those hosts.
#[must_use]
pub fn file_mode_of(metadata: &std::fs::Metadata) -> FileMode {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        FileMode::from_mode(metadata.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        FileMode::from_mode(0o644)
    }
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

/// Shortest digest [`is_cas_file_path`] accepts, the bound pnpm's
/// `isCafsFile` uses.
const CAS_DIGEST_MIN_LENGTH: usize = 40;

/// Whether `path` has the CAFS layout `files/<2 hex digits>/<digest>[-exec]`.
///
/// The package importer also materializes local-directory dependencies
/// from their project's own files. Those files keep the mode their
/// project gives them, so only a path of this shape is a store entry
/// whose mode [`store_entry_mode`] re-derives. Matches pnpm's
/// `isCafsFile`.
#[must_use]
pub fn is_cas_file_path(path: &Path) -> bool {
    fn component(path: Option<&Path>) -> Option<&str> {
        path?.file_name()?.to_str()
    }
    fn is_lower_hex(value: &str) -> bool {
        value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    }
    let Some(name) = component(Some(path)) else { return false };
    let digest = name.strip_suffix("-exec").unwrap_or(name);
    let shard_dir = path.parent();
    let Some(shard) = component(shard_dir) else { return false };
    digest.len() >= CAS_DIGEST_MIN_LENGTH
        && is_lower_hex(digest)
        && shard.len() == 2
        && is_lower_hex(shard)
        && component(shard_dir.and_then(Path::parent)) == Some("files")
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
    #[cfg(target_os = "wasi")]
    if cas_path_is_executable(cas_path) {
        let file =
            crate::ensure_file::retry_on_fd_pressure(|| crate::wasi_fs::open_nofollow(target))?;
        make_file_executable(&file)?;
    }
    #[cfg(not(any(unix, target_os = "wasi")))]
    let _ = (cas_path, target);
    Ok(())
}

/// Set Unix permission bits without following a final symlink. No-op on Windows.
pub fn set_path_permissions(path: &Path, mode: FileMode) -> io::Result<()> {
    #[cfg(unix)]
    {
        let file = crate::ensure_file::retry_on_fd_pressure(|| open_without_following(path))?;
        if FileMode::from(file.metadata()?.permissions()) != mode {
            file.set_permissions(mode.into())?;
        }
    }
    #[cfg(target_os = "wasi")]
    {
        let file =
            crate::ensure_file::retry_on_fd_pressure(|| crate::wasi_fs::open_nofollow(path))?;
        if FileMode::from_mode(crate::wasi_fs::file_mode(&file)?) != mode {
            crate::wasi_fs::set_file_mode(&file, mode.mode())?;
        }
    }
    #[cfg(not(any(unix, target_os = "wasi")))]
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
        let mode = FileMode::from(file.metadata()?.permissions());
        if mode.mode() & EXEC_MASK == EXEC_MASK {
            return Ok(());
        }
        file.set_permissions(FileMode::from_mode(mode.mode() | EXEC_MASK).into())
    };

    #[cfg(target_os = "wasi")]
    {
        let mode = FileMode::from_mode(crate::wasi_fs::file_mode(file)?);
        if mode.mode() & EXEC_MASK != EXEC_MASK {
            crate::wasi_fs::set_file_mode(file, mode.mode() | EXEC_MASK)?;
        }
        Ok(())
    }
    #[cfg(windows)]
    return Ok(());
}

/// Permission bits a new file takes from its parent directory.
///
/// Read bits and group-write come from the directory, so a group-writable
/// store stays group-writable for the next user. Other-write is never
/// copied: a world-writable sticky store protects its entries, not their
/// contents. Execute bits are copied only when
/// `executable` is set, and only for classes that can search the directory.
/// Setuid, setgid, and sticky are not copied onto a file. Owner read and
/// write are always set so the creating process can finish the write.
#[must_use]
pub fn inherited_file_mode(parent_mode: FileMode, executable: bool) -> FileMode {
    let parent_mode = parent_mode.mode();
    let mut mode = parent_mode & 0o664;
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
    FileMode::from_mode(mode | 0o600)
}

/// Open-mode ceiling for a new file, and the bits to add after create.
///
/// The open mode is a ceiling. A default ACL can grant only bits that are
/// present in it, and umask can only remove bits. [`grant_mode_bits`] adds
/// back bits umask stripped, without clearing bits the create already set.
/// An explicit private mode (no group or other bits, such as `0o600`) is
/// used as the ceiling and is not widened.
/// Any other requested mode is replaced by [`inherited_file_mode`] when
/// `parent` can be stated, keeping only whether it is executable. When
/// `parent` cannot be stated, the requested mode is used unchanged.
#[cfg(unix)]
#[must_use]
pub fn unix_creation_mode(parent: &Path, requested: Option<FileMode>) -> UnixCreationMode {
    // Group and other permission bits are the low 6 bits.
    if requested.is_some_and(|mode| mode.mode().trailing_zeros() >= 6) {
        return UnixCreationMode { open_mode: requested, grant_mode: None };
    }
    let executable = requested.is_some_and(is_executable);
    match std::fs::metadata(parent) {
        Ok(meta) => {
            let wanted = inherited_file_mode(FileMode::from(meta.permissions()), executable);
            UnixCreationMode { open_mode: Some(wanted), grant_mode: Some(wanted) }
        }
        Err(_) => UnixCreationMode { open_mode: requested, grant_mode: None },
    }
}

/// Open ceiling and the post-create grant for [`unix_creation_mode`].
#[cfg(unix)]
#[derive(Clone, Copy)]
pub struct UnixCreationMode {
    pub open_mode: Option<FileMode>,
    pub grant_mode: Option<FileMode>,
}

#[cfg(unix)]
impl UnixCreationMode {
    /// Set the open-mode ceiling on `options`.
    pub fn apply_to(&self, options: &mut std::fs::OpenOptions) {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(open_mode) = self.open_mode {
            options.mode(open_mode.mode());
        }
    }

    /// Add the post-create bits to a file opened with [`Self::apply_to`].
    pub fn grant(&self, file: &std::fs::File) -> io::Result<()> {
        match self.grant_mode {
            Some(wanted) => grant_mode_bits(file, wanted),
            None => Ok(()),
        }
    }
}

/// OR `wanted` onto `file`. Bits already present are kept, so a default ACL
/// wider than the directory mode survives. `EPERM`, `EACCES`, and `EROFS`
/// are ignored: the file is already usable by its creator, and a store entry
/// this process does not own must not fail the install.
#[cfg(unix)]
pub fn grant_mode_bits(file: &std::fs::File, wanted: FileMode) -> io::Result<()> {
    let current = match file.metadata() {
        Ok(meta) => FileMode::from(meta.permissions()).mode() & 0o777,
        Err(error) if is_unchangeable(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let merged = current | (wanted.mode() & 0o777);
    if merged == current {
        return Ok(());
    }
    match file.set_permissions(FileMode::from_mode(merged).into()) {
        Err(error) if is_unchangeable(&error) => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests;
