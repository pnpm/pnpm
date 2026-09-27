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
fn set_path_permissions_refuses_symlinks() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("target");
    let link = temporary.path().join("link");
    fs::write(&target, "data").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &link).unwrap();
    assert!(super::set_path_permissions(&link, 0o755).is_err());
    assert_eq!(
        fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
    );
    super::set_path_permissions(&target, 0o755).unwrap();
    assert_eq!(
        fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
    );
}

#[cfg(unix)]
#[test]
fn make_file_executable_sets_exec_bits() {
    use super::make_file_executable;
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let file = tmp.as_file();
    make_file_executable(file).expect("set permissions");
    let mode = file
        .metadata()
        .expect("stat")
        .permissions()
        .mode();
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

    file.set_permissions(std::fs::Permissions::from_mode(0o744))
        .expect("seed 0o744");
    make_file_executable(file).expect("fill partial exec bits");
    assert_eq!(
        file.metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
    );

    make_file_executable(file).expect("already executable");
    assert_eq!(
        file.metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
    );
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

    let mode = fs::metadata(&target)
        .expect("stat")
        .permissions()
        .mode()
        & 0o777;
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

    let mode = fs::metadata(&target)
        .expect("stat")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "non-exec CAS entry must not gain exec bits, got {mode:o}");
}

#[test]
fn inherited_file_mode_copies_directory_rw_and_keeps_owner_access() {
    assert_eq!(super::inherited_file_mode(0o2775, false) & 0o777, 0o664);
    assert_eq!(super::inherited_file_mode(0o2775, true) & 0o777, 0o775);
    assert_eq!(super::inherited_file_mode(0o755, false) & 0o777, 0o644);
    assert_eq!(super::inherited_file_mode(0o700, false) & 0o777, 0o600);
    assert_eq!(super::inherited_file_mode(0o2775, true) & 0o7000, 0);
    assert_eq!(super::inherited_file_mode(0o1777, false) & 0o777, 0o664);
    assert_eq!(super::inherited_file_mode(0o1777, true) & 0o777, 0o775);
}

#[test]
fn inherited_dir_bits_carry_group_access_with_group_write() {
    assert_eq!(super::inherited_dir_bits(0o2775), 0o2070);
    assert_eq!(super::inherited_dir_bits(0o770), 0o070);
    assert_eq!(super::inherited_dir_bits(0o2750), 0o2050);
    assert_eq!(super::inherited_dir_bits(0o755), 0);
}

/// A restrictive umask leaves a new directory at `0o700`. Granting only
/// group-write and setgid would make it writable but not searchable for the
/// group.
#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_restores_group_search_under_restrictive_umask() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let tmp = tempfile::tempdir().unwrap();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o2775)).unwrap();
    let outer = tmp.path().join("files");
    let inner = outer.join("ab");
    fs::create_dir_all(&inner).unwrap();
    for dir in [&outer, &inner] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    }

    super::grant_inherited_dir_mode(&inner, tmp.path()).unwrap();

    for dir in [&outer, &inner] {
        let mode = fs::metadata(dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o2770, "{} mode {mode:o}", dir.display());
    }
}

/// A new directory swapped for a symlink before the grant must not pass the
/// grant on to the symlink's target.
#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_does_not_follow_a_swapped_symlink() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join("store");
    let outside = tmp.path().join("outside");
    fs::create_dir(&store).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o2775)).unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).unwrap();
    let shard = store.join("ab");
    std::os::unix::fs::symlink(&outside, &shard).unwrap();

    super::grant_inherited_dir_mode(&shard, &store).unwrap_err();

    let mode = fs::metadata(&outside)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o700, "symlink target must keep its mode, got {mode:o}");
}
