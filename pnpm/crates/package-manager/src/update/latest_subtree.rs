use super::seed_policy::{UpdatePlan, UpdateScope};
use crate::fast_update_lockfile::reachable_from;
use pnpm_lockfile::{Lockfile, PkgName, PkgNameVerPeer};
use pnpm_package_manifest::DependencyGroup;
use std::collections::HashSet;

/// `--latest` with selectors re-resolves the locked subtree of every matched
/// direct dependency of the updated importer, so an unchanged parent does not
/// keep an outdated child. Unmatched direct dependencies and the peers a
/// subtree resolves keep their pins.
pub(super) fn widen_drop_targets_to_subtrees(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    matched_direct: &[(String, DependencyGroup, String)],
) {
    let Some(lockfile) = scope.lockfile.document else { return };
    let matched = matched_direct
        .iter()
        .map(|(name, _, _)| name.as_str())
        .collect::<HashSet<_>>();
    let is_matched = |alias: &PkgName| matched.contains(alias.to_string().as_str());
    let Some(reachable) = reachable_from(
        lockfile,
        |importer_id, alias| importer_id == scope.lockfile.id && is_matched(alias),
        |parent, alias| !is_peer_of(lockfile, parent, alias),
    ) else {
        return;
    };
    let unmatched = unmatched_direct_names(scope, lockfile, &matched);
    for key in reachable {
        let name = key.name.to_string();
        if !unmatched.contains(&name) {
            plan.drop_targets.insert(name, None);
        }
    }
}

/// The aliases of the unmatched direct dependencies and the package names
/// the lockfile resolves them to.
pub(super) fn unmatched_direct_names(
    scope: &UpdateScope<'_>,
    lockfile: &Lockfile,
    matched: &HashSet<&str>,
) -> HashSet<String> {
    let mut names = scope.direct
        .iter()
        .map(|(name, _, _)| name.clone())
        .filter(|name| !matched.contains(name.as_str()))
        .collect::<HashSet<_>>();
    let Some(importer) = lockfile.importers.get(scope.lockfile.id) else { return names };
    let resolved = [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .filter(|(alias, _)| names.contains(&alias.to_string()))
    .filter_map(|(alias, spec)| spec.version.resolved_key(alias))
    .map(|key| key.name.to_string())
    .collect::<Vec<_>>();
    names.extend(resolved);
    names
}

/// Whether `alias` is a peer that `parent` declares or passes through from
/// its own dependencies.
fn is_peer_of(lockfile: &Lockfile, parent: &PkgNameVerPeer, alias: &PkgName) -> bool {
    let alias = alias.to_string();
    let declared = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&parent.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref())
        .is_some_and(|peers| peers.contains_key(&alias));
    declared
        || lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(parent))
            .and_then(|snapshot| snapshot.transitive_peer_dependencies.as_ref())
            .is_some_and(|peers| peers.contains(&alias))
}
