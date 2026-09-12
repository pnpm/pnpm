use super::{
    ArtifactCleanupError, CmdShimHost, Config, Context, Diagnostic, Display, Error, FsRename,
    GlobalError, GlobalPackageInfo, HashSet, IntoDiagnostic, Path, Reporter,
    acquire_global_bin_lock, bin_names_of_other_groups, check_bin_dir, find_global_package, fs,
    get_hash_link, get_installed_bin_names, global_dirs, io, is_subdir, remove_cmd_shim,
    remove_native_shim, remove_symlink_dir, replace_global_bin_slots, restore_virtual_shims,
    should_replace_existing_package, symlink_dir, unprotected_bin_names, virtual_shims_to_restore,
    warn_global,
};

/// `pnpm remove -g`. Removes the bins, hash symlinks, and install dirs of
/// every group that contains one of the requested packages.
pub fn handle_global_remove<Reporter: self::Reporter>(
    base_config: &'static Config,
    params: &[String],
) -> miette::Result<()> {
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    let _global_bin_lock = acquire_global_bin_lock(&global_bin_dir)?;

    let groups = requested_global_groups(&global_pkg_dir, params)?;
    let protected = bin_names_of_other_groups(
        &global_pkg_dir,
        &groups.iter().map(|pkg| pkg.info.hash.clone()).collect::<HashSet<_>>(),
    )
    .wrap_err("scan global package bin ownership")?;
    let shims_to_restore = virtual_shims_to_restore(
        &groups,
        &global_bin_dir,
        &protected,
        &crate::shim_dispatch::global_shims_setting(),
    )?;
    let affected_bin_names = unprotected_bin_names(&groups, &protected);
    let mut bins_to_keep = protected;
    bins_to_keep.extend(shims_to_restore.values().flatten().cloned());
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: None,
        context: "global",
    };
    let transaction = GlobalRemovalTransaction {
        groups: &groups,
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };
    if let Some(leftover) = commit_global_removal::<CmdShimHost>(&transaction, || {
        restore_virtual_shims(&shims_to_restore, &global_bin_dir)
    })? {
        warn_global::<Reporter>(&leftover.to_string());
    }
    removed_global_install_result(cleanup_removed_global_install_dirs(&groups, &cleanup))
}

/// The groups holding the requested packages, each with the bins it owns.
/// Bins shared with (and owned by) groups that survive the removal must not
/// be unlinked, or another global package's bin would be deleted.
fn requested_global_groups(
    global_pkg_dir: &Path,
    params: &[String],
) -> miette::Result<Vec<GlobalPackageBinSnapshot>> {
    let mut groups: Vec<GlobalPackageInfo> = Vec::new();
    let mut seen = HashSet::new();
    for param in params {
        let Some(pkg) = find_global_package(global_pkg_dir, param)
            .into_diagnostic()
            .wrap_err("scan global packages")?
        else {
            return Err(GlobalError::PkgNotFound { param: param.clone() }.into());
        };
        if seen.insert(pkg.hash.clone()) {
            groups.push(pkg);
        }
    }
    groups
        .into_iter()
        .map(snapshot_global_package)
        .collect::<miette::Result<Vec<_>>>()
        .wrap_err("read global package bin ownership")
}

pub(super) struct ExistingGlobalInstalls {
    pub(super) groups_to_replace: Vec<GlobalPackageBinSnapshot>,
    pub(super) protected_bins: HashSet<String>,
}

/// A global group paired with the bin names it owns, read before anything is
/// mutated. Every destructive path decides what to unlink from this snapshot,
/// so an unreadable manifest fails the command instead of silently narrowing
/// the group's ownership mid-operation.
#[derive(Clone)]
pub(super) struct GlobalPackageBinSnapshot {
    pub(super) info: GlobalPackageInfo,
    pub(super) bin_names: Vec<String>,
}

pub(super) fn snapshot_global_package(
    info: GlobalPackageInfo,
) -> miette::Result<GlobalPackageBinSnapshot> {
    let bin_names = get_installed_bin_names(&info).map_err(miette::Report::new)?;
    Ok(GlobalPackageBinSnapshot { info, bin_names })
}

pub(super) fn collect_existing_global_installs(
    global_pkg_dir: &Path,
    aliases: &[String],
    aliases_to_replace: &[String],
) -> miette::Result<ExistingGlobalInstalls> {
    let mut groups_to_replace = Vec::new();
    let mut seen = HashSet::new();
    for alias in aliases_to_replace {
        if let Some(pkg) = find_global_package(global_pkg_dir, alias).into_diagnostic()?
            && should_replace_existing_package(&pkg, aliases, aliases_to_replace)
            && seen.insert(pkg.hash.clone())
        {
            groups_to_replace.push(pkg);
        }
    }
    let groups_to_replace = groups_to_replace
        .into_iter()
        .map(snapshot_global_package)
        .collect::<miette::Result<Vec<_>>>()?;
    let exclude = groups_to_replace.iter().map(|pkg| pkg.info.hash.clone()).collect();
    let protected_bins = bin_names_of_other_groups(global_pkg_dir, &exclude)?;
    Ok(ExistingGlobalInstalls { groups_to_replace, protected_bins })
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to remove global packages")]
struct RemovedGlobalInstallCleanupError {
    #[error(not(source))]
    #[related]
    cleanup_reports: Vec<ArtifactCleanupError>,
}

pub(super) struct GlobalInstallCleanup<'a> {
    pub(super) global_pkg_dir: &'a Path,
    pub(super) global_bin_dir: &'a Path,
    pub(super) bins_to_keep: &'a HashSet<String>,
    pub(super) hash_to_keep: Option<&'a str>,
    pub(super) context: &'static str,
}

pub(super) struct GlobalRemovalTransaction<'a> {
    pub(super) groups: &'a [GlobalPackageBinSnapshot],
    pub(super) cleanup: &'a GlobalInstallCleanup<'a>,
    pub(super) affected_bin_names: &'a HashSet<String>,
}

pub(super) trait FsGlobalRemoval: FsRename {
    fn remove_bin_slot(path: &Path) -> io::Result<()> {
        if let (Some(bin_dir), Some(name)) =
            (path.parent(), path.file_name().and_then(std::ffi::OsStr::to_str))
        {
            remove_native_shim(bin_dir, name)?;
        }
        remove_cmd_shim(path)
    }

    fn remove_hash_link(path: &Path) -> io::Result<()> {
        remove_symlink_dir(path)
    }

    fn restore_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        symlink_dir(target, link)
    }
}

pub(super) fn cleanup_replaced_global_installs(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    groups: &[GlobalPackageBinSnapshot],
    active_hash: &str,
    activated_bins: &HashSet<String>,
    protected_bins: &HashSet<String>,
    restored_bin_names: &HashSet<String>,
) -> miette::Result<Option<ArtifactCleanupError>> {
    if groups.is_empty() {
        return Ok(None);
    }
    let mut bins_to_keep = activated_bins.union(protected_bins).cloned().collect::<HashSet<_>>();
    bins_to_keep.extend(restored_bin_names.iter().cloned());
    let affected_bin_names = groups
        .iter()
        .flat_map(|group| group.bin_names.iter().cloned())
        .filter(|bin| !bins_to_keep.contains(bin))
        .collect::<HashSet<_>>();
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir,
        global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: Some(active_hash),
        context: "replaced global",
    };
    let transaction = GlobalRemovalTransaction {
        groups,
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };
    let leftover_backup = commit_global_removal::<CmdShimHost>(&transaction, || Ok(()))?;
    removed_global_install_result(cleanup_removed_global_install_dirs(groups, &cleanup))?;
    Ok(leftover_backup)
}

pub(super) fn commit_global_removal<Sys: FsGlobalRemoval>(
    transaction: &GlobalRemovalTransaction<'_>,
    replace_bins: impl FnOnce() -> miette::Result<()>,
) -> miette::Result<Option<ArtifactCleanupError>> {
    replace_global_bin_slots::<Sys>(
        transaction.cleanup.global_bin_dir,
        transaction.affected_bin_names,
        || {
            replace_bins()?;
            remove_global_install_entries::<Sys>(transaction)
        },
    )
}

pub(super) fn remove_global_install_entries<Sys: FsGlobalRemoval>(
    transaction: &GlobalRemovalTransaction<'_>,
) -> miette::Result<()> {
    let cleanup = transaction.cleanup;
    let cleanup_reports = cleanup_global_bin_names::<Sys>(transaction.affected_bin_names, cleanup);
    if !cleanup_reports.is_empty() {
        return removed_global_install_result(cleanup_reports);
    }

    let mut removed_hash_groups = Vec::new();
    for group in transaction.groups {
        match remove_global_hash_link::<Sys>(&group.info, cleanup) {
            Ok(true) => removed_hash_groups.push(&group.info),
            Ok(false) => {}
            Err(report) => {
                return removed_global_install_result(restore_removed_hash_links::<Sys>(
                    report,
                    removed_hash_groups,
                    cleanup,
                ));
            }
        }
    }
    Ok(())
}

/// Put back the hash links removed before the failure, so a partial
/// removal does not leave the surviving groups unreachable. Each
/// restore that fails is reported alongside the original failure.
fn restore_removed_hash_links<Sys: FsGlobalRemoval>(
    report: ArtifactCleanupError,
    removed_hash_groups: Vec<&GlobalPackageInfo>,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Vec<ArtifactCleanupError> {
    let mut cleanup_reports = vec![report];
    for removed_group in removed_hash_groups.into_iter().rev() {
        let hash_link = get_hash_link(cleanup.global_pkg_dir, &removed_group.hash);
        if let Err(source) = Sys::restore_hash_link(&removed_group.install_dir, &hash_link) {
            cleanup_reports.push(ArtifactCleanupError {
                context: format!(
                    "restore {} hash link at {}",
                    cleanup.context,
                    hash_link.display(),
                ),
                source,
            });
        }
    }
    cleanup_reports
}

fn cleanup_removed_global_install_dirs(
    groups: &[GlobalPackageBinSnapshot],
    cleanup: &GlobalInstallCleanup<'_>,
) -> Vec<ArtifactCleanupError> {
    groups.iter().filter_map(|group| cleanup_global_install_dir(&group.info, cleanup)).collect()
}

fn removed_global_install_result(
    mut cleanup_reports: Vec<ArtifactCleanupError>,
) -> miette::Result<()> {
    if cleanup_reports.is_empty() {
        return Ok(());
    }
    if cleanup_reports.len() == 1 {
        return Err(miette::Report::new(cleanup_reports.remove(0)));
    }
    Err(RemovedGlobalInstallCleanupError { cleanup_reports }.into())
}

fn cleanup_global_bin_names<Sys: FsGlobalRemoval>(
    bin_names: &HashSet<String>,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Vec<ArtifactCleanupError> {
    bin_names.iter().filter_map(|bin_name| cleanup_global_bin::<Sys>(bin_name, cleanup)).collect()
}

fn cleanup_global_bin<Sys: FsGlobalRemoval>(
    bin_name: &str,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Option<ArtifactCleanupError> {
    if cleanup.bins_to_keep.contains(bin_name) {
        return None;
    }
    let bin_path = cleanup.global_bin_dir.join(bin_name);
    Sys::remove_bin_slot(&bin_path).err().map(|source| ArtifactCleanupError {
        context: format!("remove {} bin at {}", cleanup.context, bin_path.display()),
        source,
    })
}

fn remove_global_hash_link<Sys: FsGlobalRemoval>(
    group: &GlobalPackageInfo,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Result<bool, ArtifactCleanupError> {
    if cleanup.hash_to_keep != Some(group.hash.as_str()) {
        let hash_link = get_hash_link(cleanup.global_pkg_dir, &group.hash);
        match Sys::remove_hash_link(&hash_link) {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ArtifactCleanupError {
                    context: format!(
                        "remove {} hash link at {}",
                        cleanup.context,
                        hash_link.display(),
                    ),
                    source,
                });
            }
        }
    }
    Ok(false)
}

fn cleanup_global_install_dir(
    group: &GlobalPackageInfo,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Option<ArtifactCleanupError> {
    if is_subdir(cleanup.global_pkg_dir, &group.install_dir) {
        match fs::remove_dir_all(&group.install_dir) {
            Ok(()) => return None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(source) => {
                return Some(ArtifactCleanupError {
                    context: format!(
                        "remove {} install directory at {}",
                        cleanup.context,
                        group.install_dir.display(),
                    ),
                    source,
                });
            }
        }
    }
    None
}
