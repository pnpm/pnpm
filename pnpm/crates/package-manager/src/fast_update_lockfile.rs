use pacquet_lockfile::{Lockfile, PkgNameVerPeer};
use std::collections::{HashSet, VecDeque};

pub(crate) fn prune_unreachable_packages(lockfile: &mut Lockfile) {
    let reachable = {
        let Some(snapshots) = lockfile.snapshots.as_ref() else { return };
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        for importer in lockfile.importers.values() {
            for dependencies in [
                importer.dependencies.as_ref(),
                importer.dev_dependencies.as_ref(),
                importer.optional_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                for (alias, spec) in dependencies {
                    if let Some(key) = spec.version.resolved_key(alias) {
                        queue.push_back(key);
                    }
                }
            }
        }
        while let Some(key) = queue.pop_front() {
            if !reachable.insert(key.clone()) {
                continue;
            }
            let Some(snapshot) = snapshots.get(&key) else { continue };
            for dependencies in
                [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                    .into_iter()
                    .flatten()
            {
                for (alias, dep_ref) in dependencies {
                    if let Some(key) = dep_ref.resolve(alias) {
                        queue.push_back(key);
                    }
                }
            }
        }
        reachable
    };
    let reachable_metadata: HashSet<_> =
        reachable.iter().map(PkgNameVerPeer::without_peer).collect();
    if let Some(snapshots) = lockfile.snapshots.as_mut() {
        snapshots.retain(|key, _| reachable.contains(key));
        if snapshots.is_empty() {
            lockfile.snapshots = None;
        }
    }
    if let Some(packages) = lockfile.packages.as_mut() {
        packages.retain(|key, _| reachable_metadata.contains(key));
        if packages.is_empty() {
            lockfile.packages = None;
        }
    }
}

/// Recompute every snapshot's `optional` flag from what still reaches it:
/// set when every path from any importer goes through an
/// `optionalDependencies` edge, cleared otherwise. An importer edge that
/// moves into or out of `optionalDependencies`, or a removal that severs
/// the last non-optional path, changes the flag for the whole subtree.
pub(crate) fn recompute_optional_flags(lockfile: &mut Lockfile) {
    let only_optionally_reached = {
        let Some(snapshots) = lockfile.snapshots.as_ref() else { return };
        // Walk `(key, reached-optionally)`: `dependencies` edges keep the
        // context, `optionalDependencies` edges always enter it.
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        for importer in lockfile.importers.values() {
            for (dependencies, optional) in [
                (importer.dependencies.as_ref(), false),
                (importer.dev_dependencies.as_ref(), false),
                (importer.optional_dependencies.as_ref(), true),
            ] {
                for (alias, spec) in dependencies.into_iter().flatten() {
                    if let Some(key) = spec.version.resolved_key(alias) {
                        queue.push_back((key, optional));
                    }
                }
            }
        }
        while let Some((key, optional)) = queue.pop_front() {
            if !visited.insert((key.clone(), optional)) {
                continue;
            }
            let Some(snapshot) = snapshots.get(&key) else { continue };
            for (dependencies, next_optional) in [
                (snapshot.dependencies.as_ref(), optional),
                (snapshot.optional_dependencies.as_ref(), true),
            ] {
                for (alias, dep_ref) in dependencies.into_iter().flatten() {
                    if let Some(key) = dep_ref.resolve(alias) {
                        queue.push_back((key, next_optional));
                    }
                }
            }
        }
        let non_optional: HashSet<_> = visited
            .iter()
            .filter_map(|(key, optional)| (!optional).then_some(key.clone()))
            .collect();
        visited
            .into_iter()
            .filter_map(|(key, optional)| (optional && !non_optional.contains(&key)).then_some(key))
            .collect::<HashSet<_>>()
    };
    if let Some(snapshots) = lockfile.snapshots.as_mut() {
        for (key, snapshot) in snapshots.iter_mut() {
            snapshot.optional = only_optionally_reached.contains(key);
        }
    }
}

/// Drop every catalog snapshot entry that no importer references any
/// more, matching what a full resolution records after the same removal.
pub(crate) fn prune_unreferenced_catalog_entries(lockfile: &mut Lockfile) {
    let Some(catalogs) = lockfile.catalogs.as_ref() else {
        return;
    };
    let stale: Vec<(String, String)> = catalogs
        .iter()
        .flat_map(|(catalog_name, entries)| {
            entries.keys().map(move |alias| (catalog_name.clone(), alias.clone()))
        })
        .filter(|(catalog_name, alias)| {
            !crate::fast_update_catalogs::catalog_entry_is_referenced(lockfile, catalog_name, alias)
        })
        .collect();
    if stale.is_empty() {
        return;
    }
    let catalogs = lockfile.catalogs.as_mut().expect("checked above");
    for (catalog_name, alias) in stale {
        if let Some(entries) = catalogs.get_mut(&catalog_name) {
            entries.remove(&alias);
            if entries.is_empty() {
                catalogs.remove(&catalog_name);
            }
        }
    }
    if catalogs.is_empty() {
        lockfile.catalogs = None;
    }
}
