use crate::fast_update_lockfile::{prune_unreachable_packages, recompute_optional_flags};
use pnpm_lockfile::{Lockfile, ProjectSnapshot, prune_undeclared_importer_deps};
use pnpm_package_manifest::PackageManifest;
use std::collections::HashMap;

/// Take back out of a merged lockfile what the fold added and the
/// manifests do not declare, returning the reconciled copy — or `None`
/// when the fold added nothing that has to go.
///
/// Every install that resolves reconciles the importers itself, but
/// `--frozen-lockfile` does not resolve, so without this a reinstated
/// entry survives to the freshness check and aborts the install. See
/// [`prune_undeclared_importer_deps`] for why only the fold's own
/// additions are eligible.
///
/// Dropping an importer edge can leave packages that nothing reaches, so
/// the graph is then settled the way an absorbed importer edit settles
/// it: unreachable snapshots and their metadata go, and the surviving
/// `optional` flags are recomputed from what still reaches them.
///
/// `manifests` maps importer id to manifest, and only the importers it
/// names are pruned — a filtered install must leave the projects it did
/// not select alone.
pub(crate) fn prune_merged_branch_lockfile(
    lockfile: &Lockfile,
    pre_merge_importers: &HashMap<String, ProjectSnapshot>,
    manifests: &[(String, &PackageManifest)],
    auto_install_peers: bool,
) -> Option<Lockfile> {
    let mut pruned = lockfile.clone();
    let mut dropped = false;
    for (importer_id, manifest) in manifests {
        if let Some(importer) = pruned.importers.get_mut(importer_id) {
            dropped |= prune_undeclared_importer_deps(
                importer,
                pre_merge_importers.get(importer_id),
                manifest,
                auto_install_peers,
            );
        }
    }
    if !dropped {
        return None;
    }
    prune_unreachable_packages(&mut pruned);
    recompute_optional_flags(&mut pruned);
    Some(pruned)
}

#[cfg(test)]
mod tests;
