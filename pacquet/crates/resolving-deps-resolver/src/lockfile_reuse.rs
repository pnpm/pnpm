//! Reuse gate: decide whether the prior lockfile already satisfies a
//! wanted dependency, so the tree walker can reuse its recorded
//! resolution + subtree instead of re-resolving from the registry.
//! Mirrors pnpm's `satisfiesWanted` / `getInfoFromLockfile` gate in
//! [`resolveDependencies.ts`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1086-L1248).
//! See `pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md`.

use std::collections::HashMap;

use node_semver::Range;
use pacquet_lockfile::{PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencySpec};

use crate::hoist_peers::satisfies_including_prerelease;

/// The snapshot key (`snapshots:` / `packages:` map key) the prior
/// lockfile resolved `alias` to in importer `importer_id`, when the
/// recorded version still satisfies the manifest's `bare_specifier`
/// (semver-satisfies, matching pnpm's `satisfiesWanted`).
///
/// Returns `None` — so the caller resolves fresh — for a new dependency,
/// an edited range the locked version no longer satisfies, a non-semver
/// `bare_specifier`, or a `link:` recorded shape. The first cut reuses
/// only semver (registry/tarball) deps; richer shapes (`link:`/`file:`/
/// `workspace:`/`catalog:`) fall through to a normal resolve.
#[allow(
    dead_code,
    reason = "consumed by the resolve_node reuse gate in the next commit of this staged feature (pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md); only the unit tests exercise it until then"
)]
pub(crate) fn reusable_importer_dep(
    importers: &HashMap<String, ProjectSnapshot>,
    importer_id: &str,
    alias: &str,
    bare_specifier: &str,
) -> Option<PkgNameVerPeer> {
    let name: PkgName = alias.parse().ok()?;
    let spec = importer_dep(importers.get(importer_id)?, &name)?;
    let version = spec.version.ver_peer()?.version_semver()?;
    let range = bare_specifier.parse::<Range>().ok()?;
    if !satisfies_including_prerelease(&range, version) {
        return None;
    }
    spec.version.resolved_key(&name)
}

/// The recorded resolution for `name` across the importer's prod /
/// optional / dev dependency maps.
fn importer_dep<'a>(
    importer: &'a ProjectSnapshot,
    name: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| importer.optional_dependencies.as_ref().and_then(|deps| deps.get(name)))
        .or_else(|| importer.dev_dependencies.as_ref().and_then(|deps| deps.get(name)))
}

#[cfg(test)]
mod tests;
