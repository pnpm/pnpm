use super::{
    super::{
        GlobalPackageBinSnapshot, cleanup_replaced_global_installs, plan_replaced_global_bins,
        restore_virtual_shims, snapshot_global_package,
    },
    FsArtifactProbe, FsRename, FsSwapHashLink, SavedBinSlot,
    activate_global_install_with_extra_bin_names, hash_linked_packages, replace_global_bin_slots,
    restore_bin_slots,
};
use crate::{
    cli_args::{
        global::{
            activation::slots::{
                BinSlotKind, directory_symlink_slots, needs_directory_symlink_removal,
            },
            remove::{
                GlobalInstallCleanup, GlobalRemovalTransaction, remove_global_install_entries,
            },
        },
        shim::{record_virtual_shim_state, virtual_shim_owner},
    },
    shim_dispatch::{ShimTarget, install_native_shim},
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

fn global_package_with_bins(
    install_dir: &Path,
    hash: &str,
    bins: &[&str],
) -> GlobalPackageBinSnapshot {
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
    snapshot_global_package(GlobalPackageInfo {
        hash: hash.to_string(),
        install_dir: install_dir.to_path_buf(),
        dependencies: vec![(alias.to_string(), "1.0.0".to_string())],
    })
    .expect("snapshot the installed group's bin ownership")
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

mod cleanup;

mod rollback;
