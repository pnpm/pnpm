use super::{EXEC_MASK, EXEC_MODE, cas_path_is_executable, is_executable};
use std::path::Path;

#[test]
fn exec_constants_pin_pnpm_layout() {
    assert_eq!(EXEC_MASK, 0o111);
    assert_eq!(EXEC_MODE, 0o755);
}

#[test]
fn is_executable_matches_any_exec_bit() {
    assert!(!is_executable(0o644));
    assert!(is_executable(0o744));
    assert!(is_executable(0o755));
    assert!(is_executable(0o050));
    assert!(is_executable(0o001));
}

#[test]
fn cas_path_is_executable_matches_trailing_suffix() {
    assert!(cas_path_is_executable(Path::new("files/1b/59d9-exec")));
    assert!(!cas_path_is_executable(Path::new("files/1b/59d9")));
    assert!(!cas_path_is_executable(Path::new("files-exec/1b/59d9")));
    assert!(!cas_path_is_executable(Path::new("files/1b/59d9-executable")));
}

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

/// The short-circuit keys on *all* exec bits being set, not merely one, so a
/// partial `0o744` is still filled to `0o755`.
#[cfg(unix)]
#[test]
fn make_file_executable_fills_partial_bits_and_preserves_full() {
    use super::make_file_executable;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let file = tmp.as_file();

    file.set_permissions(std::fs::Permissions::from_mode(0o744)).expect("seed 0o744");
    make_file_executable(file).expect("fill partial exec bits");
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o755);

    make_file_executable(file).expect("already executable");
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o755);
}

/// The `0o644` seed stands in for a target a reflink left non-executable.
#[cfg(unix)]
#[test]
fn restore_exec_bit_adds_bits_for_exec_suffix() {
    use super::restore_exec_bit_from_cas_suffix;
    use std::{fs, os::unix::fs::PermissionsExt};

    let tmp = tempfile::tempdir().expect("create tempdir");
    let cas_path = Path::new("files/1b/59d9-exec");
    let target = tmp.path().join("dst");
    fs::write(&target, b"#!/usr/bin/env node\n").expect("write target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("seed mode");

    restore_exec_bit_from_cas_suffix(cas_path, &target).expect("restore exec bit");

    let mode = fs::metadata(&target).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "exec-suffixed CAS entry must land executable, got {mode:o}");
}

/// Restoration keys on the suffix, not the mode, so it must never widen a
/// restrictive non-`-exec` target.
#[cfg(unix)]
#[test]
fn restore_exec_bit_does_not_widen_non_exec_suffix() {
    use super::restore_exec_bit_from_cas_suffix;
    use std::{fs, os::unix::fs::PermissionsExt};

    let tmp = tempfile::tempdir().expect("create tempdir");
    let cas_path = Path::new("files/1b/59d9");
    let target = tmp.path().join("dst");
    fs::write(&target, b"private data\n").expect("write target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).expect("seed mode");

    restore_exec_bit_from_cas_suffix(cas_path, &target).expect("restore is a no-op here");

    let mode = fs::metadata(&target).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "non-exec CAS entry must not gain exec bits, got {mode:o}");
}
