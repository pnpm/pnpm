use super::{
    super::{
        GlobalInstallCleanup, GlobalRemovalTransaction, cleanup_replaced_global_installs,
        plan_replaced_global_bins, remove_global_install_entries, restore_virtual_shims,
    },
    BinSlotKind, FsArtifactProbe, FsRename, FsSwapHashLink, SavedBinSlot,
    activate_global_install_with_extra_bin_names, directory_symlink_slots, hash_linked_packages,
    needs_directory_symlink_removal, replace_global_bin_slots, restore_bin_slots,
};
use miette::IntoDiagnostic;
use pnpm_cmd_shim::{
    FsCreateDirAll, FsEnsureExecutableBits, FsReadHead, FsReadToString, FsSetExecutable,
    FsWalkFiles, FsWrite, Host, PackageBinSource, link_bins_of_packages_with_excludes,
};
use pnpm_config::GlobalShims;
use pnpm_fs::{force_symlink_dir, read_symlink_dir, remove_symlink_dir};
use pnpm_global::GlobalPackageInfo;
use serde_json::json;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};
use tempfile::TempDir;

use crate::{
    cli_args::shim::{record_virtual_shim_state, virtual_shim_owner},
    shim_dispatch::{ShimTarget, install_native_shim},
};

fn activate_global_install<Sys>(
    install_dir: &Path,
    hash_link: &Path,
    global_bin_dir: &Path,
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
    link_bins: impl FnOnce() -> miette::Result<()>,
) -> miette::Result<super::Activation>
where
    Sys: FsWalkFiles + FsSwapHashLink + FsRename + FsArtifactProbe,
{
    activate_global_install_with_extra_bin_names::<Sys>(
        install_dir,
        hash_link,
        global_bin_dir,
        packages,
        bins_to_skip,
        &HashSet::new(),
        link_bins,
    )
}

macro_rules! delegate_cmd_shim_capabilities {
    ($system:ty) => {
        impl FsReadToString for $system {
            fn read_to_string(path: &Path) -> io::Result<String> {
                <Host as FsReadToString>::read_to_string(path)
            }
        }

        impl FsReadHead for $system {
            fn read_head(path: &Path, offset: u64, buffer: &mut [u8]) -> io::Result<usize> {
                <Host as FsReadHead>::read_head(path, offset, buffer)
            }
        }

        impl FsCreateDirAll for $system {
            fn create_dir_all(path: &Path) -> io::Result<()> {
                <Host as FsCreateDirAll>::create_dir_all(path)
            }
        }

        impl FsWalkFiles for $system {
            fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
                <Host as FsWalkFiles>::walk_files(path)
            }
        }

        impl FsSetExecutable for $system {
            fn set_executable(path: &Path) -> io::Result<()> {
                <Host as FsSetExecutable>::set_executable(path)
            }
        }

        impl FsEnsureExecutableBits for $system {
            fn ensure_executable_bits(path: &Path) -> io::Result<()> {
                <Host as FsEnsureExecutableBits>::ensure_executable_bits(path)
            }
        }
    };
}

macro_rules! delegate_artifact_probe {
    ($($system:ty),+ $(,)?) => {
        $(
            impl FsArtifactProbe for $system {
                fn artifact_exists(path: &Path) -> io::Result<bool> {
                    <Host as FsArtifactProbe>::artifact_exists(path)
                }
            }
        )+
    };
}

static PARTIAL_WRITE_CALLS: AtomicUsize = AtomicUsize::new(0);
static HASH_FAILURE_CALLS: AtomicUsize = AtomicUsize::new(0);
static RENAME_FAILURE_HASH_CALLS: AtomicUsize = AtomicUsize::new(0);
static BACKUP_CLEANUP_HASH_CALLS: AtomicUsize = AtomicUsize::new(0);
static ARTIFACT_PROBE_CALLS: AtomicUsize = AtomicUsize::new(0);
static ACTIVATION_CALLS: AtomicUsize = AtomicUsize::new(0);
static HASH_FAILURE_LOCK: Mutex<()> = Mutex::new(());
static BACKUP_CLEANUP_LOCK: Mutex<()> = Mutex::new(());

struct PartialWriteFailure;
struct HashSwapFailure;
struct RenameRollbackFailure;
struct BackupCleanupFailure;
struct ArtifactProbeFailure;
struct TrackingActivation;
struct FirstRestoreFailure;

delegate_cmd_shim_capabilities!(PartialWriteFailure);
delegate_cmd_shim_capabilities!(HashSwapFailure);
delegate_cmd_shim_capabilities!(RenameRollbackFailure);
delegate_cmd_shim_capabilities!(BackupCleanupFailure);
delegate_cmd_shim_capabilities!(ArtifactProbeFailure);
delegate_cmd_shim_capabilities!(TrackingActivation);
delegate_artifact_probe!(
    PartialWriteFailure,
    HashSwapFailure,
    RenameRollbackFailure,
    BackupCleanupFailure,
    TrackingActivation,
);

impl FsWrite for PartialWriteFailure {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        if PARTIAL_WRITE_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            return <Host as FsWrite>::write(path, bytes);
        }
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected shim write failure"))
    }
}

impl FsSwapHashLink for PartialWriteFailure {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        <Host as FsSwapHashLink>::swap_hash_link(target, link)
    }
}

impl FsRename for PartialWriteFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <Host as FsRename>::rename(source, target)
    }
}

impl FsWrite for HashSwapFailure {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        <Host as FsWrite>::write(path, bytes)
    }
}

impl FsSwapHashLink for HashSwapFailure {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        if HASH_FAILURE_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected hash swap failure",
            ));
        }
        <Host as FsSwapHashLink>::swap_hash_link(target, link)
    }
}

impl FsRename for HashSwapFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <Host as FsRename>::rename(source, target)
    }
}

impl FsWrite for RenameRollbackFailure {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        <Host as FsWrite>::write(path, bytes)
    }
}

impl FsSwapHashLink for RenameRollbackFailure {
    fn swap_hash_link(_target: &Path, _link: &Path) -> io::Result<()> {
        if RENAME_FAILURE_HASH_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected hash swap failure",
            ));
        }
        panic!("hash restoration must not run after backup rename fails")
    }
}

impl FsRename for RenameRollbackFailure {
    fn rename(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected backup rename failure"))
    }
}

impl FsWrite for BackupCleanupFailure {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        <Host as FsWrite>::write(path, bytes)
    }
}

/// The global bin directory whose backup directory
/// [`BackupCleanupFailure`] should wedge open. Guarded by
/// [`BACKUP_CLEANUP_LOCK`], like the call counter.
static BACKUP_BLOCKER_BIN_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn arm_backup_cleanup_blocker(global_bin_dir: &Path) {
    *BACKUP_BLOCKER_BIN_DIR.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(global_bin_dir.to_path_buf());
}

/// Swap the pending backup directory for a regular file of the same
/// name, so the recursive removal on the committed path fails and the
/// entry survives.
fn replace_backup_dir_with_file(global_bin_dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(global_bin_dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".pnpm-bin-backup-") {
            fs::remove_dir_all(entry.path())?;
            fs::write(entry.path(), b"not a directory\n")?;
        }
    }
    Ok(())
}

/// Leave a file inside the pending backup directory so removing it fails.
fn block_backup_cleanup() -> io::Result<()> {
    let guard = BACKUP_BLOCKER_BIN_DIR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(global_bin_dir) = guard.as_ref() else { return Ok(()) };
    for entry in fs::read_dir(global_bin_dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".pnpm-bin-backup-") {
            fs::write(entry.path().join("cleanup-blocker"), b"keep backup non-empty\n")?;
        }
    }
    Ok(())
}

impl FsSwapHashLink for BackupCleanupFailure {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        if BACKUP_CLEANUP_HASH_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            block_backup_cleanup()?;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected hash swap failure",
            ));
        }
        <Host as FsSwapHashLink>::swap_hash_link(target, link)
    }
}

impl FsRename for BackupCleanupFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <Host as FsRename>::rename(source, target)
    }
}

impl FsWrite for ArtifactProbeFailure {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        <BackupCleanupFailure as FsWrite>::write(path, bytes)
    }
}

impl FsSwapHashLink for ArtifactProbeFailure {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        <BackupCleanupFailure as FsSwapHashLink>::swap_hash_link(target, link)
    }
}

impl FsRename for ArtifactProbeFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <BackupCleanupFailure as FsRename>::rename(source, target)
    }
}

impl FsArtifactProbe for ArtifactProbeFailure {
    fn artifact_exists(path: &Path) -> io::Result<bool> {
        if ARTIFACT_PROBE_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected rollback artifact probe failure",
            ));
        }
        <Host as FsArtifactProbe>::artifact_exists(path)
    }
}

impl FsWrite for TrackingActivation {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        <Host as FsWrite>::write(path, bytes)
    }
}

impl FsSwapHashLink for TrackingActivation {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        ACTIVATION_CALLS.fetch_add(1, Ordering::SeqCst);
        <Host as FsSwapHashLink>::swap_hash_link(target, link)
    }
}

impl FsRename for TrackingActivation {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <Host as FsRename>::rename(source, target)
    }
}

impl FsRename for FirstRestoreFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        if target.ends_with("first") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected first-slot restore failure",
            ));
        }
        <Host as FsRename>::rename(source, target)
    }
}

#[test]
fn packages_are_addressed_through_the_hash_link() {
    let install_dir = Path::new("/global/v11/install-1");
    let hash_link = Path::new("/global/v11/hash-foo");
    let outside = PathBuf::from("/elsewhere/pkg");
    let packages = vec![
        PackageBinSource::new(install_dir.join("node_modules/tool"), Arc::new(json!({}))),
        PackageBinSource::new(outside.clone(), Arc::new(json!({}))),
    ];

    let linked = hash_linked_packages(&packages, install_dir, hash_link);

    // A shim embeds the path it was generated from, so generating it from
    // the hash link is what lets the next update switch the command over
    // by repointing that link alone.
    assert_eq!(linked[0].location, hash_link.join("node_modules/tool"));
    assert_eq!(linked[1].location, outside);
}

#[test]
fn the_hash_link_is_swapped_before_the_bins_are_linked() {
    let fixture = ActivationFixture::new(&["tool"]);

    activate_global_install::<Host>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            assert_eq!(
                resolved_hash_target(&fixture.hash_link),
                canonical(&fixture.fresh_install_dir),
                "the hash link must already point at the new install when the bins are linked",
            );
            Ok(())
        },
    )
    .expect("activate global install");
}

#[test]
fn only_directory_symlink_slots_need_pre_removal() {
    let slots = vec![
        SavedBinSlot {
            original: PathBuf::from("tool"),
            backup: PathBuf::from("backup/0"),
            kind: BinSlotKind::RegularFile,
        },
        SavedBinSlot {
            original: PathBuf::from("tool.cmd"),
            backup: PathBuf::from("backup/1"),
            kind: BinSlotKind::DirectorySymlink,
        },
        SavedBinSlot {
            original: PathBuf::from("tool.ps1"),
            backup: PathBuf::from("backup/2"),
            kind: BinSlotKind::FileSymlink,
        },
    ];

    assert_eq!(directory_symlink_slots(&slots), vec![Path::new("tool.cmd")]);
}

#[test]
fn directory_removal_uses_the_current_slot_kind() {
    assert!(!needs_directory_symlink_removal(None));
    assert!(!needs_directory_symlink_removal(Some(BinSlotKind::RegularFile)));
    assert!(!needs_directory_symlink_removal(Some(BinSlotKind::FileSymlink)));
    assert!(needs_directory_symlink_removal(Some(BinSlotKind::DirectorySymlink)));
}

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

#[test]
fn unsupported_bin_slot_fails_before_activation_and_cleans_preparation_artifacts() {
    ACTIVATION_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    let unsupported_path = fixture.global_bin_dir.join("tool");
    fs::create_dir(&unsupported_path).expect("create unsupported bin directory");

    let error = activate_global_install::<TrackingActivation>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<TrackingActivation>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("a directory bin slot must be rejected during preparation");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert_eq!(
        miette::Diagnostic::code(diagnostic).map(|code| code.to_string()),
        Some("ERR_PNPM_GLOBAL_BIN_UNSUPPORTED_TYPE".to_string()),
    );
    assert!(error.to_string().contains(&unsupported_path.display().to_string()));
    assert_eq!(ACTIVATION_CALLS.load(Ordering::SeqCst), 0);
    assert!(unsupported_path.is_dir());
    assert_eq!(resolved_hash_target(&fixture.hash_link), canonical(&fixture.old_install_dir));
    assert!(!fixture.fresh_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[test]
fn successful_activation_returns_deduped_unskipped_bins_and_removes_backup() {
    let mut fixture = ActivationFixture::new(&["tool", "skip"]);
    let skipped = fixture.seed_file_slot("skip", b"other owner\n", 0o740);
    let duplicate_dir = fixture.fresh_install_dir.join("node_modules/duplicate");
    fs::create_dir_all(duplicate_dir.join("bin")).expect("create duplicate bin directory");
    fs::write(duplicate_dir.join("bin/tool.js"), b"#!/usr/bin/env node\n")
        .expect("write duplicate bin source");
    fixture.packages.push(PackageBinSource::new(
        duplicate_dir,
        Arc::new(json!({
            "name": "duplicate",
            "version": "1.0.0",
            "bin": { "tool": "bin/tool.js" },
        })),
    ));

    let activated = activate_global_install::<Host>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::from(["skip".to_string()]),
        || {
            test_link_bins::<Host>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::from(["skip".to_string()]),
            )
        },
    )
    .expect("activate global install");

    assert_eq!(activated.activated_bins, HashSet::from(["tool".to_string()]));
    assert!(activated.leftover_backup.is_none());
    assert_eq!(slot_state(&fixture.global_bin_dir.join("skip")), skipped);
    assert_eq!(resolved_hash_target(&fixture.hash_link), canonical(&fixture.fresh_install_dir));
    assert!(fixture.old_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[test]
fn cleanup_after_activation_preserves_current_state_and_external_install() {
    let root = tempfile::tempdir().expect("create cleanup fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let old_install_dir = global_pkg_dir.join("old-install");
    let active_install_dir = global_pkg_dir.join("active-install");
    let external_install_dir = root.path().join("external-install");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::create_dir_all(&active_install_dir).expect("create active install directory");
    fs::create_dir_all(&external_install_dir).expect("create external install directory");
    let active_group = global_package_with_bins(
        &old_install_dir,
        "active-hash",
        &["activated", "survivor", "obsolete"],
    );
    let external_group = GlobalPackageInfo {
        hash: "external-hash".to_string(),
        install_dir: external_install_dir.clone(),
        dependencies: Vec::new(),
    };
    for bin_name in ["activated", "survivor", "obsolete"] {
        fs::write(global_bin_dir.join(bin_name), b"bin\n").expect("seed global bin");
    }
    let active_hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "active-hash");
    force_symlink_dir(&active_install_dir, &active_hash_link).expect("seed active hash link");

    let leftover = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[active_group, external_group],
        "active-hash",
        &HashSet::from(["activated".to_string()]),
        &HashSet::from(["survivor".to_string()]),
        &HashSet::new(),
    )
    .expect("clean up replaced installs after activation");
    assert!(leftover.is_none());

    assert!(global_bin_dir.join("activated").exists());
    assert!(global_bin_dir.join("survivor").exists());
    assert!(!global_bin_dir.join("obsolete").exists());
    assert_eq!(
        fs::canonicalize(&active_hash_link).expect("resolve active hash link"),
        fs::canonicalize(&active_install_dir).expect("resolve active install directory"),
    );
    assert!(!old_install_dir.exists());
    assert!(external_install_dir.exists());
}

#[test]
fn replacing_a_group_with_a_different_package_set_keeps_its_relinked_bins() {
    let root = tempfile::tempdir().expect("create cleanup fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let old_install_dir = global_pkg_dir.join("old-install");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    // `shared` is provided by the replaced group and by the group that just
    // took its place; `dropped` only by the replaced one.
    let replaced = global_package_with_bins(&old_install_dir, "old-hash", &["shared", "dropped"]);
    for bin_name in ["shared", "dropped"] {
        fs::write(global_bin_dir.join(bin_name), b"bin\n").expect("seed global bin");
    }
    let old_hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "old-hash");
    force_symlink_dir(&old_install_dir, &old_hash_link).expect("seed old hash link");

    let leftover = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[replaced],
        "new-hash",
        &HashSet::from(["shared".to_string()]),
        &HashSet::new(),
        &HashSet::new(),
    )
    .expect("clean up the replaced group");
    assert!(leftover.is_none());

    // Changing the set of packages changes the hash, so `shared` was
    // rewritten to point at the new one just before this ran — unlinking the
    // group it used to belong to must not take it away again.
    assert!(global_bin_dir.join("shared").exists());
    assert!(!global_bin_dir.join("dropped").exists());
    assert_eq!(
        fs::symlink_metadata(&old_hash_link).expect_err("the old hash link is gone").kind(),
        io::ErrorKind::NotFound,
    );
    assert!(!old_install_dir.exists());
}

#[test]
fn cleanup_failure_preserves_every_bin_and_the_group() {
    let root = tempfile::tempdir().expect("create cleanup error fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let install_dir = global_pkg_dir.join("old-install");
    fs::create_dir_all(global_bin_dir.join("blocked")).expect("create blocked bin directory");
    let group = global_package_with_bins(&install_dir, "active-hash", &["blocked", "stale"]);
    fs::write(global_bin_dir.join("stale"), b"stale\n").expect("seed stale bin");

    let error = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[group],
        "active-hash",
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
    )
    .expect_err("a directory cannot be removed as a bin file");

    let blocked_bin = global_bin_dir.join("blocked");
    assert!(error.to_string().contains("Cannot replace global bin slot"));
    assert!(error.to_string().contains(&blocked_bin.display().to_string()));
    assert!(global_bin_dir.join("stale").exists());
    assert!(blocked_bin.exists());
    assert!(install_dir.exists());
}

#[test]
fn replacing_a_package_that_drops_a_bin_restores_its_recorded_shim() {
    let root = tempfile::tempdir().expect("create replacement fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let install_dir = global_pkg_dir.join("old-install");
    let fresh_install_dir = global_pkg_dir.join("fresh-install");
    let package_dir = install_dir.join("node_modules/node");
    fs::create_dir_all(&package_dir).expect("create installed package directory");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::write(
        install_dir.join("package.json"),
        serde_json::to_vec(&json!({ "dependencies": { "node": "1.0.0" } }))
            .expect("serialize global group manifest"),
    )
    .expect("write global group manifest");
    fs::write(
        package_dir.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "node",
            "version": "1.0.0",
            "bin": { "node": "node.js" },
        }))
        .expect("serialize installed package manifest"),
    )
    .expect("write installed package manifest");
    fs::write(package_dir.join("node.js"), b"").expect("write package bin");
    fs::write(global_bin_dir.join("node"), b"old global bin\n").expect("seed global bin");
    record_virtual_shim_state(&global_bin_dir, "node", &["node".to_string()])
        .expect("record shim restoration state");
    let group = GlobalPackageInfo {
        hash: "old-hash".to_string(),
        install_dir: install_dir.clone(),
        dependencies: vec![("node".to_string(), "1.0.0".to_string())],
    };
    let old_hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "old-hash");
    force_symlink_dir(&install_dir, &old_hash_link).expect("seed old hash link");
    fs::create_dir_all(&fresh_install_dir).expect("create fresh install directory");

    let plan = plan_replaced_global_bins(
        std::slice::from_ref(&group),
        &global_bin_dir,
        &HashSet::new(),
        &HashSet::new(),
        &GlobalShims::default(),
    )
    .expect("plan dropped-bin replacement");
    let activation = activate_global_install_with_extra_bin_names::<Host>(
        &fresh_install_dir,
        &old_hash_link,
        &global_bin_dir,
        &[],
        &HashSet::new(),
        &plan.affected_bin_names,
        || restore_virtual_shims(&plan.shims_to_restore, &global_bin_dir),
    )
    .expect("activate replacement");
    assert!(activation.activated_bins.is_empty());

    let leftover = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[group],
        "old-hash",
        &HashSet::new(),
        &HashSet::new(),
        &plan.restored_bin_names(),
    )
    .expect("clean up replaced package");

    assert!(leftover.is_none());
    assert_eq!(
        virtual_shim_owner(&global_bin_dir.join("node")).expect("inspect restored shim").as_deref(),
        Some("node"),
    );
    assert_eq!(resolved_hash_target(&old_hash_link), canonical(&fresh_install_dir));
    assert!(!install_dir.exists());
}

#[test]
fn dropped_bin_failure_restores_its_command_and_hash_target() {
    let root = tempfile::tempdir().expect("create dropped-bin rollback fixture");
    let global_bin_dir = root.path().join("bin");
    let old_install_dir = root.path().join("old-install");
    let fresh_install_dir = root.path().join("fresh-install");
    let hash_link = root.path().join("hash-link");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::create_dir_all(&old_install_dir).expect("create old install directory");
    fs::create_dir_all(&fresh_install_dir).expect("create fresh install directory");
    fs::write(global_bin_dir.join("dropped"), b"old command\n").expect("seed old command");
    force_symlink_dir(&old_install_dir, &hash_link).expect("seed old hash link");

    let error = activate_global_install_with_extra_bin_names::<Host>(
        &fresh_install_dir,
        &hash_link,
        &global_bin_dir,
        &[],
        &HashSet::new(),
        &HashSet::from(["dropped".to_string()]),
        || Err(miette::miette!("injected restoration failure")),
    )
    .expect_err("restoration must fail");

    assert!(format!("{error:?}").contains("injected restoration failure"));
    assert_eq!(resolved_hash_target(&hash_link), canonical(&old_install_dir));
    assert_eq!(
        fs::read(global_bin_dir.join("dropped")).expect("read restored command"),
        b"old command\n",
    );
    assert!(!fresh_install_dir.exists());
}

#[test]
fn global_removal_reports_cleanup_failure_and_keeps_the_group() {
    let root = tempfile::tempdir().expect("create global removal fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let install_dir = global_pkg_dir.join("install");
    fs::create_dir_all(global_bin_dir.join("blocked")).expect("create blocked bin directory");
    let group = global_package_with_bins(&install_dir, "group-hash", &["blocked", "stale"]);
    fs::write(global_bin_dir.join("stale"), b"stale\n").expect("seed stale bin");
    let hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "group-hash");
    force_symlink_dir(&install_dir, &hash_link).expect("seed global hash link");
    let bins_to_keep = HashSet::new();
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: None,
        context: "global",
    };
    let affected_bin_names = HashSet::from(["blocked".to_string(), "stale".to_string()]);
    let transaction = GlobalRemovalTransaction {
        groups: std::slice::from_ref(&group),
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };

    let error = remove_global_install_entries::<Host>(&transaction)
        .expect_err("a directory cannot be removed as a bin file");

    assert!(error.to_string().contains("remove global bin"));
    assert!(global_bin_dir.join("blocked").is_dir());
    assert!(!global_bin_dir.join("stale").exists());
    assert!(hash_link.exists());
    assert!(group.install_dir.exists());
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

#[test]
fn rollback_failure_keeps_recovery_artifacts() {
    RENAME_FAILURE_HASH_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    fixture.seed_file_slot("tool", b"old tool\n", 0o750);

    let error = activate_global_install::<RenameRollbackFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<RenameRollbackFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the injected rollback rename must fail");

    let backup_dirs = backup_dirs(&fixture.global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    let message = error.to_string();
    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert_eq!(
        miette::Diagnostic::code(diagnostic).map(|code| code.to_string()),
        Some("ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED".to_string()),
    );
    assert!(message.contains(&backup_dirs[0].display().to_string()));
    assert!(message.contains(&fixture.fresh_install_dir.display().to_string()));
    assert!(format!("{error:?}").contains("injected hash swap failure"));
    assert!(format!("{error:?}").contains("injected backup rename failure"));
    let activation_source =
        std::error::Error::source(diagnostic).expect("activation error must be the source");
    assert!(format!("{activation_source:?}").contains("injected hash swap failure"));
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|message| message.contains("injected hash swap failure")),
    );
    assert!(fixture.fresh_install_dir.exists());
    assert!(backup_dirs[0].exists());
}

#[test]
fn fresh_cleanup_failure_reports_only_remaining_fresh_install() {
    let _guard = hash_failure_guard();
    HASH_FAILURE_CALLS.store(0, Ordering::SeqCst);
    let root = tempfile::tempdir().expect("create cleanup failure fixture");
    let global_bin_dir = root.path().join("bin");
    let fresh_install_path = root.path().join("fresh-install");
    let hash_link = root.path().join("hash-link");
    let package_dir = root.path().join("replacement");
    fs::create_dir_all(package_dir.join("bin")).expect("create package bin directory");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::write(&fresh_install_path, b"not a directory\n").expect("write non-directory install");
    fs::write(package_dir.join("bin/tool.js"), b"#!/usr/bin/env node\n")
        .expect("write package bin source");
    let packages = vec![PackageBinSource::new(
        package_dir,
        Arc::new(json!({
            "name": "replacement",
            "version": "2.0.0",
            "bin": { "tool": "bin/tool.js" },
        })),
    )];

    let error = activate_global_install::<HashSwapFailure>(
        &fresh_install_path,
        &hash_link,
        &global_bin_dir,
        &packages,
        &HashSet::new(),
        || test_link_bins::<HashSwapFailure>(&packages, &global_bin_dir, &HashSet::new()),
    )
    .expect_err("removing a regular file as an install directory must fail cleanup");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert!(miette::Diagnostic::code(diagnostic).is_none());
    assert!(backup_dirs(&global_bin_dir).is_empty());
    assert!(fresh_install_path.exists());
    let message = error.to_string();
    assert_eq!(
        message,
        format!(
            "Failed to clean up after global bin activation failed. Remaining artifacts: {}.",
            fresh_install_path.display(),
        ),
    );
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|message| message.contains("injected hash swap failure")),
    );
}

#[test]
fn a_committed_activation_reports_a_leftover_backup_instead_of_failing() {
    let _guard = backup_cleanup_guard();
    // Let the hash-link swap succeed, so the activation commits and the
    // only thing left to fail is removing the backup directory.
    BACKUP_CLEANUP_HASH_CALLS.store(1, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    arm_backup_cleanup_blocker(&fixture.global_bin_dir);

    let activation = activate_global_install::<BackupCleanupFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<BackupCleanupFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )?;
            replace_backup_dir_with_file(&fixture.global_bin_dir).into_diagnostic()
        },
    )
    .expect("a leftover backup directory must not fail a committed activation");

    assert_eq!(activation.activated_bins, HashSet::from(["tool".to_string()]));
    let leftover = activation.leftover_backup.expect("the leftover backup must be reported");
    assert!(leftover.to_string().contains("Failed to remove the global bin backup directory"));
    assert_eq!(backup_dirs(&fixture.global_bin_dir).len(), 1);
    assert_eq!(resolved_hash_target(&fixture.hash_link), canonical(&fixture.fresh_install_dir));
}

#[test]
fn backup_cleanup_failure_reports_only_remaining_backup_without_code() {
    let _guard = backup_cleanup_guard();
    BACKUP_CLEANUP_HASH_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    arm_backup_cleanup_blocker(&fixture.global_bin_dir);

    let error = activate_global_install::<BackupCleanupFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<BackupCleanupFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the non-empty backup directory must fail cleanup");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert!(miette::Diagnostic::code(diagnostic).is_none());
    let backup_dirs = backup_dirs(&fixture.global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    assert!(backup_dirs[0].exists());
    assert!(!fixture.fresh_install_dir.exists());
    let message = error.to_string();
    assert_eq!(
        message,
        format!(
            "Failed to clean up after global bin activation failed. Remaining artifacts: {}.",
            backup_dirs[0].display(),
        ),
    );
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|message| message.contains("injected hash swap failure")),
    );
}

#[test]
fn artifact_probe_failure_is_related_and_not_reported_as_a_confirmed_path() {
    let _guard = backup_cleanup_guard();
    BACKUP_CLEANUP_HASH_CALLS.store(0, Ordering::SeqCst);
    ARTIFACT_PROBE_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    arm_backup_cleanup_blocker(&fixture.global_bin_dir);

    let error = activate_global_install::<ArtifactProbeFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<ArtifactProbeFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the artifact probe failure must be preserved");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    let backup_dirs = backup_dirs(&fixture.global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    assert!(backup_dirs[0].exists());
    assert!(!fixture.fresh_install_dir.exists());
    assert_eq!(error.to_string(), "Failed to clean up after global bin activation failed.");
    let related = miette::Diagnostic::related(diagnostic)
        .expect("cleanup failures must be related diagnostics")
        .collect::<Vec<_>>();
    assert_eq!(related.len(), 2);
    assert!(related[0].to_string().contains("remove global bin backup directory"));
    assert!(related[0].to_string().contains(&backup_dirs[0].display().to_string()));
    assert!(related[1].to_string().contains("inspect remaining rollback artifact"));
    assert!(related[1].to_string().contains(&backup_dirs[0].display().to_string()));
    assert!(related[1].to_string().contains("injected rollback artifact probe failure"));
}

#[test]
fn both_cleanup_failures_report_both_errors_and_remaining_artifacts() {
    let _guard = backup_cleanup_guard();
    BACKUP_CLEANUP_HASH_CALLS.store(0, Ordering::SeqCst);
    let root = tempfile::tempdir().expect("create aggregate cleanup fixture");
    let global_bin_dir = root.path().join("bin");
    let fresh_install_path = root.path().join("fresh-install");
    let hash_link = root.path().join("hash-link");
    let package_dir = root.path().join("replacement");
    fs::create_dir_all(package_dir.join("bin")).expect("create package bin directory");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::write(&fresh_install_path, b"not a directory\n").expect("write non-directory install");
    fs::write(package_dir.join("bin/tool.js"), b"#!/usr/bin/env node\n")
        .expect("write package bin source");
    let packages = vec![PackageBinSource::new(
        package_dir,
        Arc::new(json!({
            "name": "replacement",
            "version": "2.0.0",
            "bin": { "tool": "bin/tool.js" },
        })),
    )];
    arm_backup_cleanup_blocker(&global_bin_dir);

    let error = activate_global_install::<BackupCleanupFailure>(
        &fresh_install_path,
        &hash_link,
        &global_bin_dir,
        &packages,
        &HashSet::new(),
        || test_link_bins::<BackupCleanupFailure>(&packages, &global_bin_dir, &HashSet::new()),
    )
    .expect_err("both cleanup operations must fail");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert!(miette::Diagnostic::code(diagnostic).is_none());
    let backup_dirs = backup_dirs(&global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    assert!(backup_dirs[0].exists());
    assert!(fresh_install_path.exists());
    let message = error.to_string();
    assert_eq!(
        message,
        format!(
            "Failed to clean up after global bin activation failed. Remaining artifacts: {}, {}.",
            backup_dirs[0].display(),
            fresh_install_path.display(),
        ),
    );
    let related = miette::Diagnostic::related(diagnostic)
        .expect("cleanup failures must be related diagnostics")
        .collect::<Vec<_>>();
    assert_eq!(related.len(), 2);
    let backup_error = std::error::Error::source(related[0]).expect("backup cleanup error source");
    assert!(related[0].to_string().contains("remove global bin backup directory"));
    assert!(related[0].to_string().contains(&backup_dirs[0].display().to_string()));
    assert!(related[0].to_string().contains(&backup_error.to_string()));
    let fresh_error = std::error::Error::source(related[1]).expect("fresh cleanup error source");
    assert!(related[1].to_string().contains("remove fresh global install directory"));
    assert!(related[1].to_string().contains(&fresh_install_path.display().to_string()));
    assert!(related[1].to_string().contains(&fresh_error.to_string()));
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|source| source.contains("injected hash swap failure")),
    );
}

fn test_link_bins<Sys>(
    packages: &[PackageBinSource],
    global_bin_dir: &Path,
    bins_to_skip: &HashSet<String>,
) -> miette::Result<()>
where
    Sys: FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_of_packages_with_excludes::<Sys>(
        packages,
        global_bin_dir,
        bins_to_skip,
        &pnpm_cmd_shim::LinkBinsOptions::default(),
    )
    .map_err(miette::Report::new)
}

struct ActivationFixture {
    _root: TempDir,
    global_bin_dir: PathBuf,
    old_install_dir: PathBuf,
    fresh_install_dir: PathBuf,
    hash_link: PathBuf,
    packages: Vec<PackageBinSource>,
}

impl ActivationFixture {
    fn new(bin_names: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("create activation fixture");
        let global_bin_dir = root.path().join("bin");
        let old_install_dir = root.path().join("old-install");
        let fresh_install_dir = root.path().join("fresh-install");
        let hash_link = root.path().join("hash-link");
        let package_dir = fresh_install_dir.join("node_modules/replacement");
        fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
        fs::create_dir_all(&old_install_dir).expect("create old install directory");
        fs::create_dir_all(package_dir.join("bin")).expect("create fresh package bin directory");
        fs::write(old_install_dir.join("marker"), b"old install\n")
            .expect("write old install marker");

        let mut bins = serde_json::Map::new();
        for bin_name in bin_names {
            let relative = format!("bin/{bin_name}.js");
            bins.insert((*bin_name).to_string(), json!(relative));
            fs::write(package_dir.join(&relative), format!("#!/usr/bin/env node\n// {bin_name}\n"))
                .expect("write fresh bin source");
        }
        let manifest = json!({
            "name": "replacement",
            "version": "2.0.0",
            "bin": bins,
        });
        force_symlink_dir(&old_install_dir, &hash_link).expect("seed old hash link");
        let packages = vec![PackageBinSource::new(package_dir, Arc::new(manifest))];

        Self {
            _root: root,
            global_bin_dir,
            old_install_dir,
            fresh_install_dir,
            hash_link,
            packages,
        }
    }

    fn seed_file_slot(&self, name: &str, bytes: &[u8], mode: u32) -> SlotState {
        let path = self.global_bin_dir.join(name);
        fs::write(&path, bytes).expect("write old bin slot");
        set_mode(&path, mode);
        slot_state(&path)
    }

    fn seed_link_or_file_slot(&self, name: &str) -> SlotState {
        let path = self.global_bin_dir.join(name);
        let target = self.old_install_dir.join(format!("{name}.js"));
        fs::write(&target, b"old linked target\n").expect("write old symlink target");
        #[cfg(unix)]
        {
            let relative_target = PathBuf::from("../old-install").join(format!("{name}.js"));
            std::os::unix::fs::symlink(relative_target, &path).expect("seed old bin symlink");
        }
        #[cfg(not(unix))]
        {
            fs::write(&path, b"old linked slot\n").expect("seed old bin fallback file");
        }
        slot_state(&path)
    }
}

fn global_package_with_bins(install_dir: &Path, hash: &str, bins: &[&str]) -> GlobalPackageInfo {
    let alias = "old-package";
    let package_dir = install_dir.join("node_modules").join(alias);
    fs::create_dir_all(&package_dir).expect("create installed package directory");
    let bin = bins
        .iter()
        .map(|name| ((*name).to_string(), json!(format!("bin/{name}.js"))))
        .collect::<serde_json::Map<_, _>>();
    fs::write(
        package_dir.join("package.json"),
        serde_json::to_vec(&json!({
            "name": alias,
            "version": "1.0.0",
            "bin": bin,
        }))
        .expect("serialize installed package manifest"),
    )
    .expect("write installed package manifest");
    GlobalPackageInfo {
        hash: hash.to_string(),
        install_dir: install_dir.to_path_buf(),
        dependencies: vec![(alias.to_string(), "1.0.0".to_string())],
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SlotState {
    File { bytes: Vec<u8>, mode: Option<u32> },
    Symlink { target: PathBuf },
}

fn slot_state(path: &Path) -> SlotState {
    let metadata = fs::symlink_metadata(path).expect("read bin slot metadata");
    if metadata.file_type().is_symlink() {
        return SlotState::Symlink { target: fs::read_link(path).expect("read bin symlink") };
    }
    assert!(metadata.is_file(), "expected a regular file or symlink at {}", path.display());
    SlotState::File { bytes: fs::read(path).expect("read bin slot"), mode: mode(&metadata) }
}

fn resolved_hash_target(link: &Path) -> PathBuf {
    let target = read_symlink_dir(link).expect("read hash link");
    let target = if target.is_absolute() {
        target
    } else {
        link.parent().expect("hash link parent").join(target)
    };
    canonical(&target)
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("canonicalize fixture path")
}

fn backup_dirs(global_bin_dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(global_bin_dir)
        .expect("read global bin directory")
        .map(|entry| entry.expect("read global bin entry"))
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".pnpm-bin-backup-"))
        .map(|entry| entry.path())
        .collect()
}

fn diagnostic_source_messages(diagnostic: &(dyn miette::Diagnostic + Send + Sync)) -> Vec<String> {
    let mut messages = Vec::new();
    let mut source = std::error::Error::source(diagnostic);
    while let Some(error) = source {
        messages.push(error.to_string());
        source = error.source();
    }
    messages
}

fn hash_failure_guard() -> MutexGuard<'static, ()> {
    HASH_FAILURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn backup_cleanup_guard() -> MutexGuard<'static, ()> {
    BACKUP_CLEANUP_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set bin slot mode");
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}
