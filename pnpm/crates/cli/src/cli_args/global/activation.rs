use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_cmd_shim::{
    FsWalkFiles, Host, PackageBinSource, get_bins_from_package_manifest, remove_bin,
};
use pnpm_fs::{read_symlink_dir, relative_path, remove_symlink_dir};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;

pub(super) trait FsSwapHashLink {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()>;
}

pub(super) trait FsRename {
    fn rename(source: &Path, target: &Path) -> io::Result<()>;
}

pub(super) trait FsArtifactProbe {
    fn artifact_exists(path: &Path) -> io::Result<bool>;
}

impl FsSwapHashLink for Host {
    fn swap_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        swap_hash_link_atomically(target, link)
    }
}

/// Point `link` at `target`, replacing any existing link in a single step
/// so a concurrent command never observes it missing. Windows cannot
/// rename over an existing junction, so there the replacement falls back
/// to the non-atomic remove-and-recreate.
fn swap_hash_link_atomically(target: &Path, link: &Path) -> io::Result<()> {
    if cfg!(windows) {
        return pnpm_fs::force_symlink_dir(target, link).map(|_| ());
    }
    let Some(parent) = link.parent() else {
        return pnpm_fs::force_symlink_dir(target, link).map(|_| ());
    };
    fs::create_dir_all(parent)?;
    let staged = link.with_extension(format!("{}.tmp", std::process::id()));
    match fs::remove_file(&staged) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    symlink_dir_entry(&relative_path(parent, target), &staged)?;
    fs::rename(&staged, link).inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })
}

#[cfg(unix)]
fn symlink_dir_entry(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink_dir_entry(target: &Path, link: &Path) -> io::Result<()> {
    pnpm_fs::force_symlink_dir(target, link).map(|_| ())
}

impl FsRename for Host {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        std::fs::rename(source, target)
    }
}

impl FsArtifactProbe for Host {
    fn artifact_exists(path: &Path) -> io::Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Display, Error, Diagnostic)]
enum GlobalActivationError {
    #[display(
        "Cannot replace global bin slot at {}: expected a regular file or symbolic link",
        path.display()
    )]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_UNSUPPORTED_TYPE))]
    UnsupportedType { path: PathBuf },

    #[display(
        "Failed to restore global bins after activation failed. Recovery files remain at {}; the fresh install remains at {}. Rollback error: {rollback_error}",
        backup_dir.display(),
        install_dir.display()
    )]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED))]
    RollbackFailed {
        backup_dir: PathBuf,
        install_dir: PathBuf,
        rollback_error: String,
        #[error(source)]
        #[diagnostic_source]
        activation_error: Box<dyn Diagnostic + Send + Sync>,
    },

    #[display(
        "Failed to restore global bins after replacement failed. Recovery files remain at {}. Rollback error: {rollback_error}",
        backup_dir.display()
    )]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED))]
    BinReplacementRollbackFailed {
        backup_dir: PathBuf,
        rollback_error: String,
        #[error(source)]
        #[diagnostic_source]
        replacement_error: Box<dyn Diagnostic + Send + Sync>,
    },

    #[display("Failed to restore all global bin slots")]
    BinSlotRestorationFailed {
        #[error(not(source))]
        #[related]
        failures: Vec<ArtifactCleanupError>,
    },

    #[display("Failed to clean up after global bin activation failed.{remaining_artifacts}")]
    RollbackCleanupFailed {
        remaining_artifacts: String,
        #[error(not(source))]
        #[related]
        cleanup_reports: Vec<ArtifactCleanupError>,
        #[error(source)]
        #[diagnostic_source]
        activation_error: Box<dyn Diagnostic + Send + Sync>,
    },
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("{context}: {source}")]
pub(super) struct ArtifactCleanupError {
    pub(super) context: String,
    #[error(source)]
    pub(super) source: io::Error,
}

#[derive(Debug)]
struct SavedBinSlot {
    original: PathBuf,
    backup: PathBuf,
    kind: BinSlotKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinSlotKind {
    RegularFile,
    FileSymlink,
    DirectorySymlink,
}

#[derive(Debug)]
struct PreparedGlobalInstall {
    actual_bins: BTreeMap<String, PathBuf>,
    actual_bin_names: HashSet<String>,
    affected_bin_names: HashSet<String>,
    backup_dir: TempDir,
    saved_bin_slots: Vec<SavedBinSlot>,
    old_hash_target: Option<PathBuf>,
}

/// The outcome of activating a group.
#[derive(Debug)]
pub(super) struct Activation {
    /// The commands the activated group provides.
    pub(super) activated_bins: HashSet<String>,
    /// Set when the backup directory outlived an already-committed
    /// activation. The caller warns rather than failing the command.
    pub(super) leftover_backup: Option<ArtifactCleanupError>,
}

pub(super) fn activate_global_install_with_extra_bin_names<Sys>(
    install_dir: &Path,
    hash_link: &Path,
    global_bin_dir: &Path,
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
    extra_bin_names: &HashSet<String>,
    link_bins: impl FnOnce() -> miette::Result<()>,
) -> miette::Result<Activation>
where
    Sys: FsWalkFiles + FsSwapHashLink + FsRename + FsArtifactProbe,
{
    let prepared = prepare_global_install::<Sys>(
        install_dir,
        hash_link,
        global_bin_dir,
        packages,
        bins_to_skip,
        extra_bin_names,
    )?;
    let activation_result = activate_prepared_global_install::<Sys>(
        install_dir,
        hash_link,
        global_bin_dir,
        link_bins,
        &prepared.actual_bins,
    );
    if let Err(activation_error) = activation_result {
        if let Err(rollback_error) =
            restore_global_install::<Sys>(hash_link, global_bin_dir, &prepared)
        {
            let backup_dir = prepared.backup_dir.path().to_path_buf();
            let _ = prepared.backup_dir.keep();
            return Err(GlobalActivationError::RollbackFailed {
                backup_dir,
                install_dir: install_dir.to_path_buf(),
                rollback_error: format!("{rollback_error:?}"),
                activation_error: activation_error.into(),
            }
            .into());
        }
        let mut cleanup_errors = cleanup_rolled_back_global_install(install_dir, &prepared);
        if !cleanup_errors.is_empty() {
            let remaining_artifacts =
                remaining_rollback_artifacts::<Sys>(install_dir, &prepared, &mut cleanup_errors);
            let _ = prepared.backup_dir.keep();
            return Err(GlobalActivationError::RollbackCleanupFailed {
                remaining_artifacts,
                cleanup_reports: cleanup_errors,
                activation_error: activation_error.into(),
            }
            .into());
        }
        return Err(activation_error);
    }

    let PreparedGlobalInstall { actual_bin_names, backup_dir, .. } = prepared;
    let backup_path = backup_dir.path().to_path_buf();
    // Activation is already committed, so a leftover backup directory must
    // not fail the command — but it points at a filesystem problem worth
    // surfacing.
    let leftover_backup = backup_dir.close().err().map(|source| ArtifactCleanupError {
        context: format!(
            "Failed to remove the global bin backup directory at {}",
            backup_path.display(),
        ),
        source,
    });
    Ok(Activation { activated_bins: actual_bin_names, leftover_backup })
}

/// Replace a batch of public bin slots as one recoverable operation.
/// Every original shell flavor is backed up before the callback runs, and
/// any callback failure restores the complete batch.
pub(super) fn replace_global_bin_slots<Sys>(
    global_bin_dir: &Path,
    bin_names: &HashSet<String>,
    replace_bins: impl FnOnce() -> miette::Result<()>,
) -> miette::Result<Option<ArtifactCleanupError>>
where
    Sys: FsRename,
{
    let backup_dir = tempfile::Builder::new()
        .prefix(".pnpm-bin-backup-")
        .tempdir_in(global_bin_dir)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!("create global bin backup directory in {}", global_bin_dir.display())
        })?;
    let saved_bin_slots = backup_bin_slots(bin_names, backup_dir.path(), global_bin_dir)?;
    if let Err(replacement_error) = replace_bins() {
        if let Err(rollback_error) =
            restore_bin_slots::<Sys>(global_bin_dir, bin_names, &saved_bin_slots)
        {
            let backup_dir = backup_dir.keep();
            return Err(GlobalActivationError::BinReplacementRollbackFailed {
                backup_dir,
                rollback_error: format!("{rollback_error:?}"),
                replacement_error: replacement_error.into(),
            }
            .into());
        }
        let backup_path = backup_dir.path().to_path_buf();
        return match backup_dir.close() {
            Ok(()) => Err(replacement_error),
            Err(error) => Err(replacement_error.wrap_err(format!(
                "Failed to remove the global bin backup directory at {}: {error}",
                backup_path.display(),
            ))),
        };
    }

    let backup_path = backup_dir.path().to_path_buf();
    let leftover_backup = backup_dir.close().err().map(|source| ArtifactCleanupError {
        context: format!(
            "Failed to remove the global bin backup directory at {}",
            backup_path.display(),
        ),
        source,
    });
    Ok(leftover_backup)
}

fn activate_prepared_global_install<Sys: FsSwapHashLink>(
    install_dir: &Path,
    hash_link: &Path,
    global_bin_dir: &Path,
    link_bins: impl FnOnce() -> miette::Result<()>,
    actual_bins: &BTreeMap<String, PathBuf>,
) -> miette::Result<()> {
    // Repointing the hash link is the switch-over: the shims resolve
    // through it, so every command the group already provides starts
    // running the new install here, in one step. Linking afterwards only
    // has to write the shims whose target actually changed, which for an
    // update of the same commands is none of them.
    Sys::swap_hash_link(install_dir, hash_link).into_diagnostic().wrap_err_with(|| {
        format!("link the global package install directory at {}", hash_link.display())
    })?;
    link_bins().wrap_err("link global package bins")?;
    remove_slots_of_missing_bins(global_bin_dir, actual_bins)
}

/// Drop the slots of commands the linker could not create because the file
/// the manifest points at is missing, so a replaced install leaves no shim
/// behind for a command that cannot run.
fn remove_slots_of_missing_bins(
    global_bin_dir: &Path,
    actual_bins: &BTreeMap<String, PathBuf>,
) -> miette::Result<()> {
    for (name, bin_path) in actual_bins {
        if bin_path.exists() {
            continue;
        }
        let slot = global_bin_dir.join(name);
        remove_bin(&slot)
            .into_diagnostic()
            .wrap_err_with(|| format!("remove global bin at {}", slot.display()))?;
    }
    Ok(())
}

/// The packages to link from, addressed through the group's hash link
/// instead of the generation directory it currently points at. Bin shims
/// embed the path they are generated from, so this is what makes a shim
/// survive the next update untouched.
pub(super) fn hash_linked_packages(
    packages: &[PackageBinSource],
    install_dir: &Path,
    hash_link: &Path,
) -> Vec<PackageBinSource> {
    packages
        .iter()
        .map(|package| match package.location.strip_prefix(install_dir) {
            Ok(relative) => {
                PackageBinSource::new(hash_link.join(relative), Arc::clone(&package.manifest))
            }
            Err(_) => package.clone(),
        })
        .collect()
}

fn restore_global_install<Sys>(
    hash_link: &Path,
    global_bin_dir: &Path,
    prepared: &PreparedGlobalInstall,
) -> miette::Result<()>
where
    Sys: FsSwapHashLink + FsRename,
{
    restore_bin_slots::<Sys>(
        global_bin_dir,
        &prepared.affected_bin_names,
        &prepared.saved_bin_slots,
    )?;
    if let Some(old_hash_target) = &prepared.old_hash_target {
        Sys::swap_hash_link(old_hash_target, hash_link).into_diagnostic().wrap_err_with(|| {
            format!("restore global package hash link at {}", hash_link.display())
        })?;
    } else {
        match remove_symlink_dir(hash_link) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!("remove global package hash link at {}", hash_link.display())
                });
            }
        }
    }
    Ok(())
}

fn restore_bin_slots<Sys: FsRename>(
    global_bin_dir: &Path,
    bin_names: &HashSet<String>,
    saved_bin_slots: &[SavedBinSlot],
) -> miette::Result<()> {
    let mut failures = remove_current_bin_slots(global_bin_dir, bin_names, saved_bin_slots);
    for saved_bin_slot in saved_bin_slots {
        if let Err(source) = Sys::rename(&saved_bin_slot.backup, &saved_bin_slot.original) {
            failures.push(ArtifactCleanupError {
                context: format!(
                    "restore global bin slot from {} to {}",
                    saved_bin_slot.backup.display(),
                    saved_bin_slot.original.display(),
                ),
                source,
            });
        }
    }
    if !failures.is_empty() {
        return Err(GlobalActivationError::BinSlotRestorationFailed { failures }.into());
    }
    Ok(())
}

fn cleanup_rolled_back_global_install(
    install_dir: &Path,
    prepared: &PreparedGlobalInstall,
) -> Vec<ArtifactCleanupError> {
    let mut errors = Vec::new();
    if let Err(error) = fs::remove_dir(prepared.backup_dir.path()) {
        errors.push(ArtifactCleanupError {
            context: format!(
                "remove global bin backup directory at {}",
                prepared.backup_dir.path().display(),
            ),
            source: error,
        });
    }
    if let Err(error) = remove_dir_all_if_exists(install_dir) {
        errors.push(ArtifactCleanupError {
            context: format!("remove fresh global install directory at {}", install_dir.display()),
            source: error,
        });
    }
    errors
}

fn remaining_rollback_artifacts<Sys: FsArtifactProbe>(
    install_dir: &Path,
    prepared: &PreparedGlobalInstall,
    cleanup_reports: &mut Vec<ArtifactCleanupError>,
) -> String {
    let mut artifacts = Vec::new();
    for path in [prepared.backup_dir.path(), install_dir] {
        match Sys::artifact_exists(path) {
            Ok(true) => artifacts.push(path.display().to_string()),
            Ok(false) => {}
            Err(error) => cleanup_reports.push(ArtifactCleanupError {
                context: format!("inspect remaining rollback artifact at {}", path.display()),
                source: error,
            }),
        }
    }
    if artifacts.is_empty() {
        String::new()
    } else {
        format!(" Remaining artifacts: {}.", artifacts.join(", "))
    }
}

fn remove_current_bin_slots(
    global_bin_dir: &Path,
    actual_bin_names: &HashSet<String>,
    saved_bin_slots: &[SavedBinSlot],
) -> Vec<ArtifactCleanupError> {
    let mut failures = Vec::new();
    for path in directory_symlink_slots(saved_bin_slots) {
        let current_kind = match fs::symlink_metadata(path) {
            Ok(metadata) => bin_slot_kind(&metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                failures.push(ArtifactCleanupError {
                    context: format!(
                        "read current global bin slot metadata from {}",
                        path.display(),
                    ),
                    source: error,
                });
                continue;
            }
        };
        if needs_directory_symlink_removal(current_kind) {
            match remove_symlink_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => failures.push(ArtifactCleanupError {
                    context: format!(
                        "remove directory-symlink global bin slot at {}",
                        path.display(),
                    ),
                    source,
                }),
            }
        }
    }
    for name in actual_bin_names {
        let bin_path = global_bin_dir.join(name);
        if let Err(source) = crate::shim_dispatch::remove_native_shim(global_bin_dir, name) {
            failures.push(ArtifactCleanupError {
                context: format!("remove current global shim at {}", bin_path.display()),
                source,
            });
        }
        if let Err(source) = remove_bin(&bin_path) {
            failures.push(ArtifactCleanupError {
                context: format!("remove global bin at {}", bin_path.display()),
                source,
            });
        }
    }
    failures
}

fn needs_directory_symlink_removal(current_kind: Option<BinSlotKind>) -> bool {
    current_kind == Some(BinSlotKind::DirectorySymlink)
}

fn directory_symlink_slots(saved_bin_slots: &[SavedBinSlot]) -> Vec<&Path> {
    saved_bin_slots
        .iter()
        .filter(|slot| slot.kind == BinSlotKind::DirectorySymlink)
        .map(|slot| slot.original.as_path())
        .collect()
}

fn prepare_global_install<Sys: FsWalkFiles>(
    install_dir: &Path,
    hash_link: &Path,
    global_bin_dir: &Path,
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
    extra_bin_names: &HashSet<String>,
) -> miette::Result<PreparedGlobalInstall> {
    let actual_bins = get_actual_bins::<Sys>(packages, bins_to_skip);
    let actual_bin_names: HashSet<String> = actual_bins.keys().cloned().collect();
    let affected_bin_names = actual_bin_names.union(extra_bin_names).cloned().collect();
    let backup_dir =
        match tempfile::Builder::new().prefix(".pnpm-bin-backup-").tempdir_in(global_bin_dir) {
            Ok(backup_dir) => backup_dir,
            Err(error) => {
                let report = io_error_report(
                    error,
                    format!("create global bin backup directory in {}", global_bin_dir.display()),
                );
                return cleanup_failed_preparation(install_dir, None, report);
            }
        };
    let saved_bin_slots =
        match backup_bin_slots(&affected_bin_names, backup_dir.path(), global_bin_dir) {
            Ok(saved_bin_slots) => saved_bin_slots,
            Err(error) => return cleanup_failed_preparation(install_dir, Some(backup_dir), error),
        };
    let old_hash_target = match read_hash_target(hash_link) {
        Ok(old_hash_target) => old_hash_target,
        Err(error) => return cleanup_failed_preparation(install_dir, Some(backup_dir), error),
    };
    Ok(PreparedGlobalInstall {
        actual_bins,
        actual_bin_names,
        affected_bin_names,
        backup_dir,
        saved_bin_slots,
        old_hash_target,
    })
}

fn cleanup_failed_preparation<Value>(
    install_dir: &Path,
    backup_dir: Option<TempDir>,
    preparation_error: miette::Report,
) -> miette::Result<Value> {
    let mut cleanup_errors = Vec::new();
    if let Some(backup_dir) = backup_dir {
        let backup_path = backup_dir.path().to_path_buf();
        if let Err(error) = backup_dir.close() {
            cleanup_errors.push(format!(
                "remove global bin backup directory at {}: {error}",
                backup_path.display(),
            ));
        }
    }
    if let Err(error) = remove_dir_all_if_exists(install_dir) {
        cleanup_errors.push(format!(
            "remove fresh global install directory at {}: {error}",
            install_dir.display(),
        ));
    }
    if cleanup_errors.is_empty() {
        return Err(preparation_error);
    }
    Err(preparation_error.wrap_err(format!(
        "Failed to clean up after global bin activation preparation failed: {}",
        cleanup_errors.join("; "),
    )))
}

fn io_error_report(error: io::Error, context: String) -> miette::Report {
    Err::<(), _>(error).into_diagnostic().wrap_err(context).unwrap_err()
}

fn remove_dir_all_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The commands the group declares, mapped to the file each one runs.
fn get_actual_bins<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
) -> BTreeMap<String, PathBuf> {
    packages
        .iter()
        .flat_map(|package| {
            get_bins_from_package_manifest::<Sys>(&package.manifest, &package.location)
        })
        .filter(|command| !bins_to_skip.contains(&command.name))
        .map(|command| (command.name, command.path))
        .collect()
}

pub(super) fn get_actual_bin_names<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
) -> HashSet<String> {
    get_actual_bins::<Sys>(packages, bins_to_skip).into_keys().collect()
}

fn backup_bin_slots(
    actual_bin_names: &HashSet<String>,
    backup_dir: &Path,
    global_bin_dir: &Path,
) -> miette::Result<Vec<SavedBinSlot>> {
    let mut saved_bin_slots = Vec::new();
    for (index, original) in
        actual_bin_names.iter().flat_map(|name| bin_slot_paths(global_bin_dir, name)).enumerate()
    {
        let backup = backup_dir.join(index.to_string());
        if let Some(saved_bin_slot) = backup_bin_slot(original, backup)? {
            saved_bin_slots.push(saved_bin_slot);
        }
    }
    Ok(saved_bin_slots)
}

/// Every file a bin slot can occupy: the shell flavors cmd-shim writes and
/// the native shim's executable and sidecar.
fn bin_slot_paths(global_bin_dir: &Path, name: &str) -> Vec<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) { &["", ".cmd", ".ps1"] } else { &[""] };
    let mut paths: Vec<PathBuf> = extensions
        .iter()
        .map(|extension| global_bin_dir.join(format!("{name}{extension}")))
        .collect();
    paths.extend(crate::shim_dispatch::native_shim_paths(global_bin_dir, name));
    paths.dedup();
    paths
}

fn backup_bin_slot(original: PathBuf, backup: PathBuf) -> miette::Result<Option<SavedBinSlot>> {
    let metadata = match fs::symlink_metadata(&original) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).into_diagnostic().wrap_err_with(|| {
                format!("read global bin slot metadata from {}", original.display())
            });
        }
    };
    let kind = bin_slot_kind(&metadata)
        .ok_or_else(|| GlobalActivationError::UnsupportedType { path: original.clone() })?;
    match kind {
        BinSlotKind::FileSymlink | BinSlotKind::DirectorySymlink => {
            backup_symlink(&original, &backup, kind).into_diagnostic().wrap_err_with(|| {
                format!("back up global bin symlink at {}", original.display())
            })?;
        }
        BinSlotKind::RegularFile => backup_regular_file(&original, &backup, metadata.permissions())
            .into_diagnostic()
            .wrap_err_with(|| format!("back up global bin file at {}", original.display()))?,
    }
    Ok(Some(SavedBinSlot { original, backup, kind }))
}

fn backup_regular_file(
    original: &Path,
    backup: &Path,
    permissions: fs::Permissions,
) -> io::Result<()> {
    if reflink_copy::reflink(original, backup).is_err() {
        match fs::remove_file(backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::copy(original, backup)?;
    }
    fs::set_permissions(backup, permissions)
}

#[cfg(unix)]
fn bin_slot_kind(metadata: &fs::Metadata) -> Option<BinSlotKind> {
    if metadata.file_type().is_symlink() {
        Some(BinSlotKind::FileSymlink)
    } else if metadata.is_file() {
        Some(BinSlotKind::RegularFile)
    } else {
        None
    }
}

#[cfg(windows)]
fn bin_slot_kind(metadata: &fs::Metadata) -> Option<BinSlotKind> {
    use std::os::windows::fs::FileTypeExt;
    let file_type = metadata.file_type();
    if file_type.is_symlink_file() {
        Some(BinSlotKind::FileSymlink)
    } else if file_type.is_symlink_dir() {
        Some(BinSlotKind::DirectorySymlink)
    } else if metadata.is_file() {
        Some(BinSlotKind::RegularFile)
    } else {
        None
    }
}

#[cfg(not(any(unix, windows)))]
fn bin_slot_kind(metadata: &fs::Metadata) -> Option<BinSlotKind> {
    metadata.is_file().then_some(BinSlotKind::RegularFile)
}

#[cfg(unix)]
fn backup_symlink(original: &Path, backup: &Path, _kind: BinSlotKind) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(original)?, backup)
}

#[cfg(windows)]
fn backup_symlink(original: &Path, backup: &Path, kind: BinSlotKind) -> io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let target = fs::read_link(original)?;
    match kind {
        BinSlotKind::FileSymlink => symlink_file(target, backup),
        BinSlotKind::DirectorySymlink => symlink_dir(target, backup),
        BinSlotKind::RegularFile => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "regular files cannot be backed up as symlinks",
        )),
    }
}

fn read_hash_target(hash_link: &Path) -> miette::Result<Option<PathBuf>> {
    match read_symlink_dir(hash_link) {
        Ok(target) if target.is_absolute() => Ok(Some(target)),
        Ok(target) => Ok(Some(
            hash_link.parent().map_or_else(|| target.clone(), |parent| parent.join(&target)),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).into_diagnostic().wrap_err_with(|| {
            format!("read existing global package hash link at {}", hash_link.display())
        }),
    }
}

#[cfg(test)]
mod tests;
