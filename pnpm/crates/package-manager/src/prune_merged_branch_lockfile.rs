use crate::fast_update_lockfile::{prune_unreachable_packages, recompute_optional_flags};
use pnpm_lockfile::{
    Lockfile, ProjectSnapshot, importer_link_target, prune_undeclared_importer_deps,
    reconcile_merged_importer_metadata, restore_moved_links,
};
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
    let merged_link_targets: HashMap<String, String> = manifests
        .iter()
        .filter_map(|(importer_id, _)| {
            let importer = pruned.importers.get(importer_id)?;
            Some((importer_id.clone(), importer_link_target(importer_id, importer)))
        })
        .collect();
    let mut deps_dropped = false;
    let mut metadata_reconciled = false;
    for (importer_id, manifest) in manifests {
        if let Some(importer) = pruned.importers.get_mut(importer_id) {
            let pre_merge = pre_merge_importers.get(importer_id);
            deps_dropped |=
                prune_undeclared_importer_deps(importer, pre_merge, manifest, auto_install_peers);
            let reconciliation = reconcile_merged_importer_metadata(importer, pre_merge, manifest);
            deps_dropped |= reconciliation.edges_changed;
            metadata_reconciled |= reconciliation.changed;
        }
    }
    metadata_reconciled |= restore_moved_links(
        &mut pruned.importers,
        pre_merge_importers,
        &merged_link_targets,
        manifests,
    );
    if !deps_dropped && !metadata_reconciled {
        return None;
    }
    if deps_dropped {
        prune_unreachable_packages(&mut pruned);
        recompute_optional_flags(&mut pruned);
    }
    Some(pruned)
}

#[cfg(test)]
mod tests;
