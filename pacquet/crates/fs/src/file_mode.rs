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

#[cfg(test)]
mod tests;
