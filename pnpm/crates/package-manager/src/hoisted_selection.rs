use pnpm_lockfile::{Lockfile, ProjectSnapshot};
use std::collections::HashSet;

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

fn records_a_dependency(snapshot: &ProjectSnapshot) -> bool {
    snapshot
        .dependencies_by_groups(crate::DIRECT_GROUPS)
        .next()
        .is_some()
}

#[cfg(test)]
mod tests;
