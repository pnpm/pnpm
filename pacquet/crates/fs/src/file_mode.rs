use std::io;

/// Bit mask to filter executable bits (`--x--x--x`).
pub const EXEC_MASK: u32 = 0b001_001_001;

/// All can read and execute, but only owner can write (`rwxr-xr-x`).
pub const EXEC_MODE: u32 = 0b111_101_101;

/// Whether a file mode has *any* executable bit set (`u+x`, `g+x`, or
/// `o+x`). Matches pnpm's `modeIsExecutable` and is therefore the rule
/// pacquet must follow when deciding whether a CAFS blob gets the
/// `-exec` suffix or has its on-disk mode flipped executable. Tarballs
/// from npm frequently ship scripts as `0o744` (user exec only) or
/// `0o755` (all); both must be treated as executable for pnpm-interop.
#[must_use]
pub fn is_executable(mode: u32) -> bool {
    mode & EXEC_MASK != 0
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
