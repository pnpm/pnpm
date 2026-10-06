use super::{EXEC_MASK, EXEC_MODE, cas_path_is_executable, is_cas_file_path, is_executable};
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

#[test]
fn is_cas_file_path_matches_the_store_layout_only() {
    let digest = "59d9".repeat(10);
    let store = Path::new("store/v11/files/1b");
    assert!(is_cas_file_path(&store.join(&digest)));
    assert!(is_cas_file_path(&store.join(format!("{digest}-exec"))));
    assert!(is_cas_file_path(&store.join("a".repeat(126))));

    assert!(!is_cas_file_path(&store.join(&digest[1..])), "digest shorter than a SHA-1");
    assert!(!is_cas_file_path(&store.join(digest.to_uppercase())), "uppercase hex");
    assert!(!is_cas_file_path(&store.join(format!("{digest}.js"))), "not a bare digest");
    assert!(!is_cas_file_path(&Path::new("files/1b0").join(&digest)), "shard of three digits");
    assert!(!is_cas_file_path(&Path::new("files/zz").join(&digest)), "non-hex shard");
    assert!(!is_cas_file_path(&Path::new("dist/1b").join(&digest)), "not under files/");
    assert!(!is_cas_file_path(&Path::new("1b").join(&digest)), "no files/ ancestor");
    assert!(!is_cas_file_path(Path::new("project/bin/pnpm-exec")), "a project's own file");
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

/// pnpm/pnpm#16677: what a store write gives a file in a group- or
/// world-writable store directory is linkable under the umask that
/// readable-and-writable store implies.
#[test]
fn store_inode_mode_is_linkable_ignores_group_and_other_write() {
    use super::{store_entry_mode, store_inode_mode_is_linkable};
    for (dir_mode, umask) in [(0o777, 0o000), (0o1777, 0o000), (0o2775, 0o002), (0o2775, 0o022)] {
        for executable in [false, true] {
            let mode = super::inherited_file_mode(dir_mode, executable);
            let desired = store_entry_mode(executable, umask);
            assert!(
                store_inode_mode_is_linkable(mode, desired),
                "a {mode:o} entry from a {dir_mode:o} directory is linkable under umask {umask:o}",
            );
        }
    }
}

#[test]
fn store_inode_mode_is_linkable_rejects_other_differences() {
    use super::{store_entry_mode, store_inode_mode_is_linkable};
    let plain_077 = store_entry_mode(false, 0o077);
    let exec_077 = store_entry_mode(true, 0o077);
    assert!(!store_inode_mode_is_linkable(0o644, plain_077), "extra read bits");
    assert!(!store_inode_mode_is_linkable(0o755, exec_077), "extra read and execute bits");
    assert!(!store_inode_mode_is_linkable(0o646, plain_077), "extra other-write");
    assert!(
        !store_inode_mode_is_linkable(0o666, store_entry_mode(false, 0o022)),
        "extra other-write",
    );
    assert!(
        !store_inode_mode_is_linkable(0o600, store_entry_mode(false, 0o022)),
        "missing read bits",
    );
    assert!(
        !store_inode_mode_is_linkable(0o644, store_entry_mode(true, 0o022)),
        "missing execute bits",
    );
    assert!(
        !store_inode_mode_is_linkable(0o755, store_entry_mode(false, 0o022)),
        "extra execute bits",
    );
    assert!(
        !store_inode_mode_is_linkable(0o444, store_entry_mode(false, 0o022)),
        "missing owner write",
    );
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

/// A new directory swapped for a FIFO before the grant must be refused
/// without blocking on the FIFO's open.
#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_refuses_a_swapped_fifo_without_blocking() {
    use std::{ffi::CString, fs, os::unix::ffi::OsStrExt, os::unix::fs::PermissionsExt};

    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join("store");
    fs::create_dir(&store).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o2775)).unwrap();
    let shard = store.join("ab");
    let c_path = CString::new(shard.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c_path` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

    let error = super::grant_inherited_dir_mode(&shard, &store).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
}

/// A umask such as `0o477` leaves a new directory its owner cannot open.
/// The grant must still reach it.
#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_reaches_a_directory_its_owner_cannot_read() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let tmp = tempfile::tempdir().unwrap();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o2775)).unwrap();
    let shard = tmp.path().join("ab");
    fs::create_dir(&shard).unwrap();
    fs::set_permissions(&shard, fs::Permissions::from_mode(0o300)).unwrap();

    super::grant_inherited_dir_mode(&shard, tmp.path()).unwrap();

    let mode = fs::metadata(&shard)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o2370, "mode {mode:o}");
}

#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_rejects_unrelated_template_without_changes() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let temporary = tempfile::tempdir().unwrap();
    let template = temporary.path().join("template");
    let child = temporary.path().join("unrelated/child");
    fs::create_dir(&template).unwrap();
    fs::create_dir_all(&child).unwrap();
    fs::set_permissions(&template, fs::Permissions::from_mode(0o2770)).unwrap();
    for directory in [temporary.path(), child.parent().unwrap(), &child] {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    }

    let error = super::grant_inherited_dir_mode(&child, &template).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    for directory in [temporary.path(), child.parent().unwrap(), &child] {
        assert_eq!(
            fs::metadata(directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700,
        );
    }
}

#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_rejects_symlink_parent_escape() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    let temporary = tempfile::tempdir().unwrap();
    let template = temporary.path().join("template");
    let target = temporary.path().join("outside/target");
    let victim = temporary.path().join("outside/victim");
    fs::create_dir(&template).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::create_dir(&victim).unwrap();
    fs::set_permissions(&template, fs::Permissions::from_mode(0o2770)).unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&target, template.join("link")).unwrap();

    let error =
        super::grant_inherited_dir_mode(&template.join("link/../victim"), &template).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        fs::metadata(&victim)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700,
    );
}

#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_refuses_intermediate_symlink() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    let temporary = tempfile::tempdir().unwrap();
    let template = temporary.path().join("store");
    let outside = temporary.path().join("outside");
    let victim = outside.join("victim");
    fs::create_dir(&template).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir(&victim).unwrap();
    fs::set_permissions(&template, fs::Permissions::from_mode(0o2775)).unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&outside, template.join("link")).unwrap();

    let error =
        super::grant_inherited_dir_mode(&template.join("link/victim"), &template).unwrap_err();
    assert!(
        error.kind() == std::io::ErrorKind::NotADirectory
            || error.raw_os_error() == Some(libc::ELOOP),
    );
    for directory in [&outside, &victim] {
        assert_eq!(
            fs::metadata(directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700,
            "{} mode changed through a symlink",
            directory.display(),
        );
    }
}

#[cfg(unix)]
#[test]
fn create_dir_all_inheriting_mode_rejects_parent_traversal_before_creation() {
    use std::fs;

    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    fs::create_dir(&store).unwrap();
    let path = store.join("new/../created");

    let error = super::create_dir_all_inheriting_mode(&path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!store.join("new").exists());
    assert!(!store.join("created").exists());
}

#[cfg(unix)]
#[test]
fn create_dir_all_inheriting_mode_accepts_existing_parent_traversal() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let temporary = tempfile::tempdir().unwrap();
    fs::create_dir(temporary.path().join("workspace")).unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o2775)).unwrap();
    let directory = temporary.path().join("workspace/../store/files");

    super::create_dir_all_inheriting_mode(&directory).unwrap();
    assert_eq!(
        fs::metadata(&directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o2775,
    );
}

#[cfg(unix)]
#[test]
fn grant_inherited_dir_mode_accepts_relative_dot_template() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let temporary = tempfile::tempdir_in(".").unwrap();
    let directory = temporary.path().join("child");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let template_mode = fs::metadata(".")
        .unwrap()
        .permissions()
        .mode();
    let absolute = std::path::absolute(&directory).unwrap();
    let cwd = std::env::current_dir().unwrap();
    let relative = absolute.strip_prefix(&cwd).unwrap();

    super::grant_inherited_dir_mode(relative, Path::new(".")).unwrap();
    assert_eq!(
        fs::metadata(&directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700 | super::inherited_dir_bits(template_mode),
    );
    assert_eq!(
        fs::metadata(".")
            .unwrap()
            .permissions()
            .mode(),
        template_mode,
    );
}
