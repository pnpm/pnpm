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

/// Set file mode to 777 on POSIX platforms such as Linux or macOS,
/// or do nothing on Windows.
#[cfg_attr(windows, allow(unused))]
pub fn make_file_executable(file: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    return {
        use std::{
            fs::Permissions,
            os::unix::fs::{MetadataExt, PermissionsExt},
        };
        let mode = file.metadata()?.mode();
        let mode = mode | EXEC_MASK;
        let permissions = Permissions::from_mode(mode);
        file.set_permissions(permissions)
    };

    #[cfg(windows)]
    return Ok(());
}

#[cfg(test)]
mod tests;
