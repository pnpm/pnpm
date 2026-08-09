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
