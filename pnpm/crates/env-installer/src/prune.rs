//! Drop `packages:` / `snapshots:` entries the env lockfile's
//! importers no longer reference.
//!
//! The env document is smaller than a normal project lockfile, so this
//! walks the snapshot graph directly rather than running a general
//! lockfile pruner over it.

use pnpm_lockfile::{EnvLockfile, PackageKey, SnapshotEntry};
use std::collections::HashSet;

/// Retain only the `packages:` / `snapshots:` entries reachable from
/// `importers["."]`.
pub fn prune_env_lockfile(env: &mut EnvLockfile) {
    let mut reachable: HashSet<PackageKey> = HashSet::new();
    let mut pending: Vec<PackageKey> = direct_keys(env);

    while let Some(key) = pending.pop() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = env.snapshots.get(&key) else {
            continue;
        };
        pending.extend(snapshot_keys(snapshot));
    }

    env.packages.retain(|key, _| reachable.contains(key));
    env.snapshots.retain(|key, _| reachable.contains(key));
}

/// The package keys the root importer depends on directly. A specifier that
/// does not parse as a key names nothing in `snapshots:`, so it is dropped.
fn direct_keys(env: &EnvLockfile) -> Vec<PackageKey> {
    let Some(importer) = env.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) else {
        return Vec::new();
    };
    importer
        .config_dependencies
        .iter()
        .chain(importer.package_manager_dependencies.iter().flatten())
        .filter_map(|(name, spec)| format!("{name}@{}", spec.version).parse::<PackageKey>().ok())
        .collect()
}

fn snapshot_keys(snapshot: &SnapshotEntry) -> Vec<PackageKey> {
    [&snapshot.dependencies, &snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|(subdep_name, dep_ref)| dep_ref.resolve(subdep_name))
        .collect()
}
