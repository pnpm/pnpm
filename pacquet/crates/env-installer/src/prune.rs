//! Drop `packages:` / `snapshots:` entries the env lockfile's
//! importers no longer reference. Mirrors pnpm's
//! [`pruneEnvLockfile`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/pruneEnvLockfile.ts),
//! which runs the env document through the shared lockfile pruner.
//!
//! The env document is smaller than a normal project lockfile, so this
//! walks the snapshot graph directly instead of porting the general
//! `pruneSharedLockfile`.

use pacquet_lockfile::{EnvLockfile, PackageKey};
use std::collections::HashSet;

/// Retain only the `packages:` / `snapshots:` entries reachable from
/// `importers["."]`.
pub fn prune_env_lockfile(env: &mut EnvLockfile) {
    let mut reachable: HashSet<PackageKey> = HashSet::new();
    let mut pending: Vec<PackageKey> = Vec::new();

    if let Some(importer) = env.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) {
        let direct = importer
            .config_dependencies
            .iter()
            .chain(importer.package_manager_dependencies.iter().flatten());
        for (name, spec) in direct {
            let Ok(key) = format!("{name}@{}", spec.version).parse::<PackageKey>() else {
                continue;
            };
            pending.push(key);
        }
    }

    while let Some(key) = pending.pop() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = env.snapshots.get(&key) else {
            continue;
        };
        for deps in [&snapshot.dependencies, &snapshot.optional_dependencies].into_iter().flatten()
        {
            for (subdep_name, dep_ref) in deps {
                if let Some(subkey) = dep_ref.resolve(subdep_name) {
                    pending.push(subkey);
                }
            }
        }
    }

    env.packages.retain(|key, _| reachable.contains(key));
    env.snapshots.retain(|key, _| reachable.contains(key));
}
