use super::{EXEC_MASK, EXEC_MODE, cas_path_is_executable, is_executable};
use std::path::Path;

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

/// Only a trailing `-exec` segment on the file name marks a CAS path
/// executable. The suffix must be exactly the file-name tail pnpm's
/// CAFS layout writes — a `-exec` elsewhere in the path, or a name that
/// merely contains the substring, does not count.
#[test]
fn cas_path_is_executable_matches_trailing_suffix() {
    assert!(cas_path_is_executable(Path::new("files/1b/59d9-exec")));
    assert!(!cas_path_is_executable(Path::new("files/1b/59d9")));
    // `-exec` only in a parent directory, not the file name.
    assert!(!cas_path_is_executable(Path::new("files-exec/1b/59d9")));
    // Substring, not a trailing segment.
    assert!(!cas_path_is_executable(Path::new("files/1b/59d9-executable")));
}

/// `make_file_executable` flips the exec bits on a freshly
/// created non-executable file. Only meaningful on Unix — the
/// Windows arm of the function is a no-op and there is no
/// permissions field to observe.
#[cfg(unix)]
#[test]
fn make_file_executable_sets_exec_bits() {
    use super::make_file_executable;
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let file = tmp.as_file();
    make_file_executable(file).expect("set permissions");
    let mode = file.metadata().expect("stat").permissions().mode();
    assert_eq!(mode & EXEC_MASK, EXEC_MASK, "all exec bits should be set, got {mode:o}");
}
