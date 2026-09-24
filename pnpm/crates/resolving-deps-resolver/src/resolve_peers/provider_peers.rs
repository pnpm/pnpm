//! The peers a package declares, checked against the versions an
//! importer provides — so an optional peer provider taken from elsewhere
//! in the graph is not hoisted where its own peers would conflict.

use super::context;
use crate::{resolve_dependency_tree::WorkspaceTreeCtx, resolved_tree::ResolvedPackage};
use pnpm_resolving_resolver_base::get_peer_version_range;
use rustc_hash::FxHashMap as HashMap;

/// The version `package` installs as, the one a peer range is checked against.
pub(crate) fn resolved_version(package: &ResolvedPackage) -> String {
    context::pkg_name_version(&package.result).1
}

/// `peer_name → range` for each peer the package `name@version`
/// declares. The package is looked up by name and version, since one
/// resolved from a named registry has a different id. One seeded only
/// from the wanted lockfile has no resolved package yet, so the lockfile
/// describes its peers instead. `None` when neither knows the package.
pub(crate) fn declared_peer_ranges(
    workspace: &WorkspaceTreeCtx,
    name: &str,
    version: &str,
) -> Option<Vec<(String, String)>> {
    let ranges_of = |package: &ResolvedPackage| {
        package.peer_dependencies
            .iter()
            .map(|(peer_name, peer)| (peer_name.clone(), peer.version.clone()))
            .collect()
    };
    let pkg_id = format!("{name}@{version}");
    workspace
        .inspect_package(&pkg_id, ranges_of)
        .or_else(|| {
            workspace.inspect_matching_package(
                |package| {
                    let (pkg_name, pkg_version) = context::pkg_name_version(&package.result);
                    pkg_name == name && pkg_version == version
                },
                ranges_of,
            )
        })
        .or_else(|| {
            let key = pkg_id.parse::<pnpm_lockfile::PkgNameVerPeer>().ok()?;
            let metadata = workspace
                .wanted_lockfile()?
                .packages
                .as_ref()?
                .get(&key)?;
            Some(
                metadata.peer_dependencies
                    .iter()
                    .flatten()
                    .map(|(peer_name, range)| (peer_name.clone(), range.clone()))
                    .collect(),
            )
        })
}

/// Whether each of `peer_ranges` accepts the version that
/// `provided_versions` maps its peer to. A peer with no entry passes.
pub(crate) fn peers_accept_provided_versions(
    peer_ranges: &[(String, String)],
    provided_versions: &HashMap<String, String>,
) -> bool {
    peer_ranges
        .iter()
        .all(|(peer_name, range)| {
            provided_versions
                .get(peer_name)
                .is_none_or(|version| {
                    context::satisfies_with_prereleases(version, &get_peer_version_range(range))
                })
        })
}
