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
mod tests {
    use super::{EXEC_MASK, EXEC_MODE, is_executable, make_file_executable};

    /// Sanity-pin the two on-disk constants. The mask is `--x--x--x`
    /// and the canonical executable mode is `rwxr-xr-x` — these
    /// match pnpm's `EXEC_MODE` and are part of the CAFS contract.
    #[test]
    fn exec_constants_pin_pnpm_layout() {
        assert_eq!(EXEC_MASK, 0o111);
        assert_eq!(EXEC_MODE, 0o755);
    }

    /// Every tarball-shipped exec bit (`u+x`, `g+x`, `o+x`) flips
    /// `is_executable` to `true`. Any-bit semantics matches
    /// upstream's `modeIsExecutable`.
    #[test]
    fn is_executable_matches_any_exec_bit() {
        assert!(!is_executable(0o644));
        assert!(is_executable(0o744)); // user-only exec, the common npm shape
        assert!(is_executable(0o755));
        assert!(is_executable(0o050)); // group-only — still executable
        assert!(is_executable(0o001)); // other-only — still executable
    }

    /// `make_file_executable` flips the exec bits on a freshly
    /// created non-executable file. On Unix the mode must include
    /// all three exec bits afterward; on Windows the call is a
    /// no-op and we just assert it returns `Ok`.
    #[test]
    fn make_file_executable_sets_exec_bits_on_unix() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let file = tmp.as_file();
        make_file_executable(file).expect("set permissions");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = file.metadata().expect("stat").permissions().mode();
            assert_eq!(mode & EXEC_MASK, EXEC_MASK, "all exec bits should be set, got {mode:o}");
        }
    }
}
