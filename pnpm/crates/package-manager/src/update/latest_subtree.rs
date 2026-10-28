use super::seed_policy::{UpdatePlan, UpdateScope};
use crate::fast_update_lockfile::reachable_from;
use pnpm_lockfile::{Lockfile, PkgName, PkgNameVerPeer};
use pnpm_package_manifest::DependencyGroup;
use std::collections::HashSet;

/// `--latest` with selectors re-resolves the locked subtree of every matched
/// direct dependency, so an unchanged parent does not keep an outdated child.
/// Unmatched direct dependencies and the peers a subtree resolves keep their
/// pins.
pub(super) fn widen_drop_targets_to_subtrees(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    matched_direct: &[(String, DependencyGroup, String)],
) {
    let Some(lockfile) = scope.lockfile else { return };
    let matched = matched_direct
        .iter()
        .map(|(name, _, _)| name.as_str())
        .collect::<HashSet<_>>();
    let Some(reachable) = reachable_from(
        lockfile,
        |alias| matched.contains(alias.to_string().as_str()),
        |parent, alias| !is_peer_of(lockfile, parent, alias),
    ) else {
        return;
    };
    let unmatched = scope.direct
        .iter()
        .map(|(name, _, _)| name.as_str())
        .filter(|name| !matched.contains(name))
        .collect::<HashSet<_>>();
    for key in reachable {
        let name = key.name.to_string();
        if !unmatched.contains(name.as_str()) {
            plan.drop_targets.insert(name, None);
        }
    }
}

fn is_peer_of(lockfile: &Lockfile, parent: &PkgNameVerPeer, alias: &PkgName) -> bool {
    lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&parent.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref())
        .is_some_and(|peers| peers.contains_key(&alias.to_string()))
}
