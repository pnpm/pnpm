use super::{
    ActivationFixture, BinSlotKind, FirstRestoreFailure, HASH_FAILURE_CALLS, HashSet,
    HashSwapFailure, Host, IntoDiagnostic, Ordering, PARTIAL_WRITE_CALLS, PartialWriteFailure,
    SavedBinSlot, ShimTarget, activate_global_install, backup_dirs, canonical, fs,
    hash_failure_guard, install_native_shim, io, read_symlink_dir, remove_symlink_dir,
    replace_global_bin_slots, resolved_hash_target, restore_bin_slots, slot_state, test_link_bins,
};
#[cfg(windows)]
use std::path::PathBuf;

#[test]
fn partial_shim_failure_restores_exact_slots_and_hash_target() {
    PARTIAL_WRITE_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["first", "second", "shared"]);
    let first = fixture.seed_file_slot("first", b"old first\n", 0o751);
    let second = fixture.seed_link_or_file_slot("second");
    let shared = fixture.seed_file_slot("shared", b"other owner\n", 0o740);

    let error = activate_global_install::<PartialWriteFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::from(["shared".to_string()]),
        || {
            test_link_bins::<PartialWriteFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::from(["shared".to_string()]),
            )
        },
    )
    .expect_err("the injected shim write must fail activation");

    assert!(format!("{error:?}").contains("injected shim write failure"));
    assert_eq!(slot_state(&fixture.global_bin_dir.join("first")), first);
    assert_eq!(slot_state(&fixture.global_bin_dir.join("second")), second);
    assert_eq!(slot_state(&fixture.global_bin_dir.join("shared")), shared);
    assert_eq!(resolved_hash_target(&fixture.hash_link), canonical(&fixture.old_install_dir));
    assert!(fixture.old_install_dir.exists());
    assert!(!fixture.fresh_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[test]
fn failed_batch_bin_replacement_restores_earlier_slots() {
    let fixture = ActivationFixture::new(&["first", "second"]);
    let first = fixture.seed_file_slot("first", b"old first\n", 0o751);
    let second = fixture.seed_link_or_file_slot("second");
    let bin_names = HashSet::from(["first".to_string(), "second".to_string()]);

    let error = replace_global_bin_slots::<Host>(&fixture.global_bin_dir, &bin_names, || {
        install_native_shim(
            &fixture.global_bin_dir,
            "first",
            &ShimTarget::Virtual("first-package".to_string()),
        )
        .into_diagnostic()?;
        Err(miette::miette!("injected later replacement failure"))
    })
    .expect_err("the injected replacement must fail");

    assert!(format!("{error:?}").contains("injected later replacement failure"));
    assert_eq!(slot_state(&fixture.global_bin_dir.join("first")), first);
    assert_eq!(slot_state(&fixture.global_bin_dir.join("second")), second);
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[test]
fn rollback_continues_after_bin_removal_failure() {
    let root = tempfile::tempdir().expect("create rollback fixture");
    let global_bin_dir = root.path().join("bin");
    let backup_dir = root.path().join("backup");
    fs::create_dir_all(global_bin_dir.join("first")).expect("create obstructing bin directory");
    fs::create_dir_all(&backup_dir).expect("create backup directory");
    fs::write(global_bin_dir.join("second"), b"replacement second\n")
        .expect("write replacement bin");
    fs::write(backup_dir.join("first"), b"old first\n").expect("write first backup");
    fs::write(backup_dir.join("second"), b"old second\n").expect("write second backup");
    let saved_bin_slots = vec![
        SavedBinSlot {
            original: global_bin_dir.join("first"),
            backup: backup_dir.join("first"),
            kind: BinSlotKind::RegularFile,
        },
        SavedBinSlot {
            original: global_bin_dir.join("second"),
            backup: backup_dir.join("second"),
            kind: BinSlotKind::RegularFile,
        },
    ];

    let error = restore_bin_slots::<Host>(
        &global_bin_dir,
        &HashSet::from(["first".to_string(), "second".to_string()]),
        &saved_bin_slots,
    )
    .expect_err("the obstructing directory must prevent complete rollback");

    assert!(format!("{error:?}").contains("remove global bin"));
    assert!(global_bin_dir.join("first").is_dir());
    assert_eq!(
        fs::read(global_bin_dir.join("second")).expect("read restored second bin"),
        b"old second\n",
    );
    assert!(backup_dir.join("first").exists());
    assert!(!backup_dir.join("second").exists());
}

#[test]
fn rollback_continues_after_backup_rename_failure() {
    let root = tempfile::tempdir().expect("create rollback fixture");
    let global_bin_dir = root.path().join("bin");
    let backup_dir = root.path().join("backup");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::create_dir_all(&backup_dir).expect("create backup directory");
    for name in ["first", "second"] {
        fs::write(global_bin_dir.join(name), format!("replacement {name}\n"))
            .expect("write replacement bin");
        fs::write(backup_dir.join(name), format!("old {name}\n")).expect("write bin backup");
    }
    let saved_bin_slots = vec![
        SavedBinSlot {
            original: global_bin_dir.join("first"),
            backup: backup_dir.join("first"),
            kind: BinSlotKind::RegularFile,
        },
        SavedBinSlot {
            original: global_bin_dir.join("second"),
            backup: backup_dir.join("second"),
            kind: BinSlotKind::RegularFile,
        },
    ];

    let error = restore_bin_slots::<FirstRestoreFailure>(
        &global_bin_dir,
        &HashSet::from(["first".to_string(), "second".to_string()]),
        &saved_bin_slots,
    )
    .expect_err("the injected rename must prevent complete rollback");

    assert!(format!("{error:?}").contains("injected first-slot restore failure"));
    assert!(!global_bin_dir.join("first").exists());
    assert_eq!(
        fs::read(global_bin_dir.join("second")).expect("read restored second bin"),
        b"old second\n",
    );
    assert!(backup_dir.join("first").exists());
    assert!(!backup_dir.join("second").exists());
}

#[test]
fn hash_swap_failure_restores_bins_and_hash_target() {
    let _guard = hash_failure_guard();
    HASH_FAILURE_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    let tool = fixture.seed_file_slot("tool", b"old tool\n", 0o750);
    assert!(
        !read_symlink_dir(&fixture.hash_link).expect("read relative hash target").is_absolute(),
    );

    let error = activate_global_install::<HashSwapFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<HashSwapFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the injected hash activation must fail");

    assert!(format!("{error:?}").contains("injected hash swap failure"));
    assert_eq!(slot_state(&fixture.global_bin_dir.join("tool")), tool);
    assert_eq!(resolved_hash_target(&fixture.hash_link), canonical(&fixture.old_install_dir));
    assert!(fixture.old_install_dir.exists());
    assert!(!fixture.fresh_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[test]
fn hash_failure_removes_hash_link_that_was_originally_absent() {
    let _guard = hash_failure_guard();
    HASH_FAILURE_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    remove_symlink_dir(&fixture.hash_link).expect("remove initial hash link");

    let error = activate_global_install::<HashSwapFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<HashSwapFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the injected hash activation must fail");

    assert!(format!("{error:?}").contains("injected hash swap failure"));
    assert_eq!(
        fs::symlink_metadata(&fixture.hash_link).expect_err("hash link remains absent").kind(),
        io::ErrorKind::NotFound,
    );
    assert!(!fixture.fresh_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[cfg(windows)]
#[test]
fn hash_failure_restores_windows_file_and_directory_symlink_kinds() {
    let _guard = hash_failure_guard();
    use std::os::windows::fs::{FileTypeExt, symlink_dir, symlink_file};

    HASH_FAILURE_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["file-link", "dir-link"]);
    let file_target = PathBuf::from("../old-install/file-target.js");
    let dir_target = PathBuf::from("../old-install/dir-target");
    fs::write(fixture.old_install_dir.join("file-target.js"), b"old file target\n")
        .expect("write file symlink target");
    fs::create_dir_all(fixture.old_install_dir.join("dir-target"))
        .expect("create directory symlink target");
    let file_link = fixture.global_bin_dir.join("file-link");
    let dir_link = fixture.global_bin_dir.join("dir-link");
    symlink_file(&file_target, &file_link).expect("seed file symlink");
    symlink_dir(&dir_target, &dir_link).expect("seed directory symlink");

    let error = activate_global_install::<HashSwapFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<HashSwapFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the injected hash activation must fail");

    assert!(format!("{error:?}").contains("injected hash swap failure"));
    let file_type = fs::symlink_metadata(&file_link).expect("file link metadata").file_type();
    assert!(file_type.is_symlink_file());
    assert_eq!(fs::read_link(&file_link).expect("read file symlink"), file_target);
    let dir_type = fs::symlink_metadata(&dir_link).expect("dir link metadata").file_type();
    assert!(dir_type.is_symlink_dir());
    assert_eq!(fs::read_link(&dir_link).expect("read directory symlink"), dir_target);
    assert!(!fixture.fresh_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}
