//! Drop `packages:` / `snapshots:` entries the env lockfile's
//! importers no longer reference. Mirrors pnpm's
//! [`pruneEnvLockfile`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/pruneEnvLockfile.ts),
//! which runs the env document through the shared lockfile pruner.
//!
//! The env document's reachability graph is shallow — config (and
//! package-manager) deps plus one level of their optional subdeps — so
//! this walks that graph directly instead of porting the general
//! `pruneSharedLockfile`.

use pacquet_lockfile::{EnvLockfile, PackageKey};
use std::collections::HashSet;

/// Retain only the `packages:` / `snapshots:` entries reachable from
/// `importers["."]`.
pub fn prune_env_lockfile(env: &mut EnvLockfile) {
    let mut reachable: HashSet<PackageKey> = HashSet::new();

    if let Some(importer) = env.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) {
        let direct = importer
            .config_dependencies
            .iter()
            .chain(importer.package_manager_dependencies.iter().flatten());
        for (name, spec) in direct {
            let Ok(key) = format!("{name}@{}", spec.version).parse::<PackageKey>() else {
                continue;
            };
            // Pull in one level of optional subdeps before moving the
            // key, so the borrow on `env.snapshots` ends first.
            if let Some(snapshot) = env.snapshots.get(&key)
                && let Some(optionals) = snapshot.optional_dependencies.as_ref()
            {
                for (subdep_name, dep_ref) in optionals {
                    if let Some(ver_peer) = dep_ref.ver_peer()
                        && let Ok(subkey) =
                            format!("{subdep_name}@{ver_peer}").parse::<PackageKey>()
                    {
                        reachable.insert(subkey);
                    }
                }
            }
            reachable.insert(key);
        }
    }

    env.packages.retain(|key, _| reachable.contains(key));
    env.snapshots.retain(|key, _| reachable.contains(key));
}
