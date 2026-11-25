use super::{
    GlobalInstallCleanup,
    GlobalPackageBinSnapshot,
    GlobalRemovalTransaction,
    commit_global_removal,
    complete_global_removal,
    protected_bins_for_removal,
    snapshot_global_package,
};
use crate::{
    cli_args::global::{
        CmdShimHost,
        GlobalPackageInfo,
        Reporter,
        restore_virtual_shims,
        unprotected_bin_names,
        virtual_shims_to_restore,
        warn_global,
    },
    shim_dispatch::global_shims_setting,
};
use std::path::Path;

/// Remove selected groups while the caller holds the global bin lock, so
/// another process cannot replace a selected hash link during the removal.
pub fn remove_global_package_groups_locked<Reporter: self::Reporter>(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    packages: &[GlobalPackageInfo],
) -> miette::Result<()> {
    let groups = packages
        .iter()
        .cloned()
        .map(snapshot_global_package)
        .collect::<miette::Result<Vec<_>>>()?;
    remove_prepared_global_groups::<Reporter>(global_pkg_dir, global_bin_dir, &groups)
}

pub(super) fn remove_prepared_global_groups<Reporter: self::Reporter>(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    groups: &[GlobalPackageBinSnapshot],
) -> miette::Result<()> {
    let skills = super::super::skills::RemovedSkillLinks::prepare(global_pkg_dir, groups)?;
    let protected = protected_bins_for_removal(global_pkg_dir, groups)?;
    let shims_to_restore =
        virtual_shims_to_restore(groups, global_bin_dir, &protected, &global_shims_setting())?;
    let affected_bin_names = unprotected_bin_names(groups, &protected);
    let mut bins_to_keep = protected;
    bins_to_keep.extend(
        shims_to_restore
            .values()
            .flatten()
            .cloned(),
    );
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir,
        global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: None,
        context: "global",
    };
    let transaction = GlobalRemovalTransaction {
        groups,
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };
    if let Some(leftover) = commit_global_removal::<CmdShimHost>(&transaction, || {
        restore_virtual_shims(&shims_to_restore, global_bin_dir)
    })? {
        warn_global::<Reporter>(leftover.to_string());
    }
    complete_global_removal(groups, &cleanup, skills)
}
