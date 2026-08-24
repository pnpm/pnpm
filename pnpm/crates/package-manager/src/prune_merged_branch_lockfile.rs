use crate::fast_update_lockfile::{prune_unreachable_packages, recompute_optional_flags};
use pnpm_lockfile::{Lockfile, prune_undeclared_importer_deps};
use pnpm_package_manifest::PackageManifest;

/// Reconcile a lockfile that `mergeGitBranchLockfiles` folded the branch
/// lockfiles into against the manifests.
///
/// The merge unions the keys of the lockfiles it folds together, so it
/// has no way to express a deletion: a dependency the main branch removed
/// after a branch lockfile was written is reinstated by the merge. Every
/// install that resolves prunes it again, but `--frozen-lockfile` does
/// not resolve, so there the reinstated entry survives to the freshness
/// check and aborts the install.
///
/// Dropping an importer edge can leave packages that nothing reaches, so
/// the graph is settled the way an absorbed importer edit settles it:
/// unreachable snapshots and their metadata go, and the surviving
/// `optional` flags are recomputed from what still reaches them. The
/// union can strand a package on its own, without any edge being dropped,
/// so the settling is not conditional on the importer pass finding
/// anything.
///
/// `manifests` maps importer id to manifest, and only the importers it
/// names are pruned — a filtered install must leave the projects it did
/// not select alone.
pub(crate) fn prune_merged_branch_lockfile(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
    auto_install_peers: bool,
) -> Lockfile {
    let mut pruned = lockfile.clone();
    for (importer_id, manifest) in manifests {
        if let Some(importer) = pruned.importers.get_mut(importer_id) {
            prune_undeclared_importer_deps(importer, manifest, auto_install_peers);
        }
    }
    prune_unreachable_packages(&mut pruned);
    recompute_optional_flags(&mut pruned);
    pruned
}

#[cfg(test)]
mod tests;
