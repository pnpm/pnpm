pub(super) use slots::{get_actual_bin_names, replace_global_bin_slots};

use derive_more::{Display, Error};
use filesystem::swap_hash_link_atomically;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_cmd_shim::{
    FsWalkFiles, Host, PackageBinSource, get_bins_from_package_manifest, remove_bin,
};
use pnpm_fs::{read_symlink_dir, relative_path, remove_symlink_dir};
use slots::{
    SavedBinSlot, backup_bin_slots, get_actual_bins, read_hash_target,
    remove_slots_of_missing_bins, restore_bin_slots,
};

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
        return rollback_failed_activation::<Sys>(
            install_dir,
            hash_link,
            global_bin_dir,
            prepared,
            activation_error,
        );
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

#[cfg(test)]
mod tests;

fn rollback_failed_activation<Sys>(
    install_dir: &Path,
    hash_link: &Path,
    global_bin_dir: &Path,
    prepared: PreparedGlobalInstall,
    activation_error: miette::Report,
) -> miette::Result<Activation>
where
    Sys: FsWalkFiles + FsSwapHashLink + FsRename + FsArtifactProbe,
{
    if let Err(rollback_error) = restore_global_install::<Sys>(hash_link, global_bin_dir, &prepared)
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
    Err(activation_error)
}

mod slots;

mod filesystem;
