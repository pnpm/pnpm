use super::{
    ArtifactCleanupError, BTreeMap, Context, FsRename, FsWalkFiles, GlobalActivationError, HashSet,
    IntoDiagnostic, PackageBinSource, Path, PathBuf, fs, get_bins_from_package_manifest, io,
    read_symlink_dir, remove_bin, remove_symlink_dir,
};

#[derive(Debug)]
pub(super) struct SavedBinSlot {
    pub(super) original: PathBuf,
    pub(super) backup: PathBuf,
    pub(super) kind: BinSlotKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinSlotKind {
    RegularFile,
    FileSymlink,
    DirectorySymlink,
}

/// Replace a batch of public bin slots as one recoverable operation.
/// Every original shell flavor is backed up before the callback runs, and
/// any callback failure restores the complete batch.
pub(in super::super) fn replace_global_bin_slots<Sys>(
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

/// Drop the slots of commands the linker could not create because the file
/// the manifest points at is missing, so a replaced install leaves no shim
/// behind for a command that cannot run.
pub(super) fn remove_slots_of_missing_bins(
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

pub(super) fn restore_bin_slots<Sys: FsRename>(
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

fn remove_current_bin_slots(
    global_bin_dir: &Path,
    actual_bin_names: &HashSet<String>,
    saved_bin_slots: &[SavedBinSlot],
) -> Vec<ArtifactCleanupError> {
    let mut failures = Vec::new();
    for path in directory_symlink_slots(saved_bin_slots) {
        remove_directory_symlink_slot(path, &mut failures);
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

/// Remove one saved directory-symlink bin slot, if it is still one.
/// A slot that vanished under us needs no removal.
fn remove_directory_symlink_slot(path: &Path, failures: &mut Vec<ArtifactCleanupError>) {
    let current_kind = match fs::symlink_metadata(path) {
        Ok(metadata) => bin_slot_kind(&metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            failures.push(ArtifactCleanupError {
                context: format!("read current global bin slot metadata from {}", path.display()),
                source: error,
            });
            return;
        }
    };
    if !needs_directory_symlink_removal(current_kind) {
        return;
    }
    match remove_symlink_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => failures.push(ArtifactCleanupError {
            context: format!("remove directory-symlink global bin slot at {}", path.display()),
            source,
        }),
    }
}

pub(super) fn needs_directory_symlink_removal(current_kind: Option<BinSlotKind>) -> bool {
    current_kind == Some(BinSlotKind::DirectorySymlink)
}

pub(super) fn directory_symlink_slots(saved_bin_slots: &[SavedBinSlot]) -> Vec<&Path> {
    saved_bin_slots
        .iter()
        .filter(|slot| slot.kind == BinSlotKind::DirectorySymlink)
        .map(|slot| slot.original.as_path())
        .collect()
}

/// The commands the group declares, mapped to the file each one runs.
pub(super) fn get_actual_bins<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
) -> BTreeMap<String, PathBuf> {
    let declared = packages.iter().flat_map(|package| {
        get_bins_from_package_manifest::<Sys>(&package.manifest, &package.location)
    });
    declared
        .filter(|command| !bins_to_skip.contains(&command.name))
        .map(|command| (command.name, command.path))
        .collect()
}

pub(in super::super) fn get_actual_bin_names<Sys: FsWalkFiles>(
    packages: &[PackageBinSource],
    bins_to_skip: &HashSet<String>,
) -> HashSet<String> {
    get_actual_bins::<Sys>(packages, bins_to_skip).into_keys().collect()
}

pub(super) fn backup_bin_slots(
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

pub(super) fn read_hash_target(hash_link: &Path) -> miette::Result<Option<PathBuf>> {
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
