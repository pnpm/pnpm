use pnpm_lockfile::{Lockfile, ProjectSnapshot};
use std::{collections::HashSet, path::Path};

/// The importers a selected (`--filter`) install materializes.
///
/// Under `nodeLinker: hoisted` every importer shares one `node_modules`
/// tree, and the install prunes whatever the new hoist leaves out. So a
/// selected hoisted install also keeps each importer whose dependencies
/// `hoisted_prior`, the previous install's current lockfile, records, as long
/// as the wanted `lockfile` still has that importer. An importer no install
/// materialized a dependency for stays out, so the selection still decides
/// what a first install brings in.
///
/// Pass `hoisted_prior` only for a hoisted layout. Other layouts give each
/// importer its own `node_modules`, so the selection alone is complete.
pub(crate) fn selected_materialization_ids(
    lockfile: &Lockfile,
    selected: &HashSet<String>,
    hoisted_prior: Option<&Lockfile>,
) -> HashSet<String> {
    let previously_materialized = hoisted_prior
        .into_iter()
        .flat_map(|prior| &prior.importers)
        .filter(|(importer_id, snapshot)| {
            records_a_dependency(snapshot) && lockfile.importers.contains_key(*importer_id)
        })
        .map(|(importer_id, _)| importer_id);
    selected
        .iter()
        .chain(previously_materialized)
        .cloned()
        .collect()
}

/// The materialization closure of a selected (`--filter`) install.
///
/// Applies `groups` (`--prod`, `--dev`, `--no-optional`) to `selected` and
/// any importers reached from `selected`, while preserving the dependency
/// groups `hoisted_prior` already materialized for unselected importers.
pub(crate) fn selected_materialization_closure(
    lockfile: &Lockfile,
    workspace_root: &Path,
    selected: &HashSet<String>,
    hoisted_prior: Option<&Lockfile>,
    groups: &crate::GroupSelection,
    skipped: &crate::SkippedSnapshots,
) -> crate::MaterializationClosure {
    let mut selected_closure =
        crate::materialization_closure(lockfile, workspace_root, selected, groups, skipped);
    let Some((retained_source, retained_ids)) = hoisted_prior.and_then(|prior| {
        retained_prior_importers(lockfile, prior, &selected_closure)
    }) else {
        return selected_closure;
    };
    let retained_closure = crate::materialization_closure(
        &retained_source,
        workspace_root,
        &retained_ids,
        &crate::GroupSelection::all(),
        &skipped.without_optional_excluded(),
    );
    merge_retained_closure(&mut selected_closure, retained_closure, groups);
    selected_closure
}

fn retained_prior_importers(
    lockfile: &Lockfile,
    prior: &Lockfile,
    selected_closure: &crate::MaterializationClosure,
) -> Option<(Lockfile, HashSet<String>)> {
    let mut retained_source = lockfile.clone();
    let mut retained_ids = HashSet::new();
    retained_source.importers = prior.importers
        .iter()
        .filter(|(importer_id, prior_snapshot)| {
            !selected_closure.importer_ids.contains(*importer_id)
                && records_a_dependency(prior_snapshot)
        })
        .filter_map(|(importer_id, prior_snapshot)| {
            let wanted_snapshot = lockfile.importers.get(importer_id)?;
            retained_ids.insert(importer_id.clone());
            Some((importer_id.clone(), retain_prior_groups(wanted_snapshot, prior_snapshot)))
        })
        .collect();
    (!retained_ids.is_empty()).then_some((retained_source, retained_ids))
}

fn merge_retained_closure(
    selected_closure: &mut crate::MaterializationClosure,
    retained_closure: crate::MaterializationClosure,
    groups: &crate::GroupSelection,
) {
    selected_closure.importer_ids.extend(retained_closure.importer_ids);
    let mut importers = retained_closure.lockfile.importers;
    importers.extend(std::mem::take(&mut selected_closure.lockfile.importers));
    selected_closure.lockfile.importers = importers;
    if let Some(retained_snapshots) = retained_closure.lockfile.snapshots {
        merge_retained_snapshots(
            &mut selected_closure.lockfile.snapshots,
            retained_snapshots,
            groups,
        );
    }
    if let Some(mut retained_packages) = retained_closure.lockfile.packages {
        if let Some(selected_packages) = selected_closure.lockfile.packages.take() {
            retained_packages.extend(selected_packages);
        }
        selected_closure.lockfile.packages = Some(retained_packages);
    }
}

fn merge_retained_snapshots(
    selected_snapshots: &mut Option<
        std::collections::HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::SnapshotEntry>,
    >,
    mut retained_snapshots: std::collections::HashMap<
        pnpm_lockfile::PackageKey,
        pnpm_lockfile::SnapshotEntry,
    >,
    groups: &crate::GroupSelection,
) {
    if let Some(selected) = selected_snapshots.take() {
        retained_snapshots.extend(selected);
    }
    groups.skipped_peer_edges.prune_dangling(&mut retained_snapshots);
    *selected_snapshots = Some(retained_snapshots);
}

fn retain_prior_groups(wanted: &ProjectSnapshot, prior: &ProjectSnapshot) -> ProjectSnapshot {
    ProjectSnapshot {
        dependencies: wanted.dependencies.clone().filter(|_| prior.dependencies.is_some()),
        dev_dependencies: wanted.dev_dependencies
            .clone()
            .filter(|_| prior.dev_dependencies.is_some()),
        optional_dependencies: wanted.optional_dependencies
            .clone()
            .filter(|_| prior.optional_dependencies.is_some()),
        ..wanted.clone()
    }
}

fn records_a_dependency(snapshot: &ProjectSnapshot) -> bool {
    snapshot
        .dependencies_by_groups(crate::DIRECT_GROUPS)
        .next()
        .is_some()
}

#[cfg(test)]
mod tests;
