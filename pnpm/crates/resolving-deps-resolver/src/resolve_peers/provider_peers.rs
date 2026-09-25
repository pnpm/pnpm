//! The peers a package declares, checked against the versions an
//! importer provides — so an optional peer provider taken from elsewhere
//! in the graph is not hoisted where its own peers would conflict.

use super::context;
use crate::{resolve_dependency_tree::WorkspaceTreeCtx, resolved_tree::ResolvedPackage};
use pnpm_resolving_resolver_base::get_peer_version_range;
use rustc_hash::FxHashMap as HashMap;
use std::cell::OnceCell;

/// The package's real name and installed version.
pub(crate) fn resolved_name_and_version(package: &ResolvedPackage) -> (String, String) {
    context::pkg_name_version(&package.result)
}

/// `peer_name → range` pairs one package declares.
type PeerRanges = Vec<(String, String)>;

/// Looks up the peers an optional peer candidate declares. A candidate is
/// found by name and version, since a package resolved from a named
/// registry has a different id. One seeded only from the wanted lockfile
/// has no resolved package yet, so the lockfile describes its peers
/// instead. The name-and-version index is built on the first miss and
/// reused for the rest of the selection.
pub(crate) struct CandidatePeerRanges<'workspace> {
    workspace: &'workspace WorkspaceTreeCtx,
    by_name_version: OnceCell<HashMap<String, PeerRanges>>,
}

impl<'workspace> CandidatePeerRanges<'workspace> {
    pub(crate) fn new(workspace: &'workspace WorkspaceTreeCtx) -> Self {
        CandidatePeerRanges { workspace, by_name_version: OnceCell::new() }
    }

    /// The peers `name@version` declares, or `None` when neither the
    /// resolved packages nor the wanted lockfile know it.
    pub(crate) fn get(&self, name: &str, version: &str) -> Option<PeerRanges> {
        let pkg_id = format!("{name}@{version}");
        if let Some(ranges) = self.workspace.inspect_package(&pkg_id, peer_ranges_of) {
            return Some(ranges);
        }
        self.by_name_version()
            .get(&pkg_id)
            .cloned()
    }

    fn by_name_version(&self) -> &HashMap<String, PeerRanges> {
        self.by_name_version.get_or_init(|| {
            let mut index = HashMap::default();
            if let Some(packages) = self.workspace
                .wanted_lockfile()
                .and_then(|lockfile| lockfile.packages.as_ref())
            {
                for (key, metadata) in packages {
                    let version = lockfile_entry_version(key, metadata);
                    index.insert(format!("{}@{version}", key.name), lockfile_peer_ranges(metadata));
                }
            }
            self.workspace.for_each_package(|package| {
                let (name, version) = context::pkg_name_version(&package.result);
                index.insert(format!("{name}@{version}"), peer_ranges_of(package));
            });
            index
        })
    }
}

fn lockfile_entry_version(
    key: &pnpm_lockfile::PkgNameVerPeer,
    metadata: &pnpm_lockfile::PackageMetadata,
) -> String {
    if let Some(v) = &metadata.version {
        return v.clone();
    }
    match key.suffix.version() {
        pnpm_lockfile::VersionPart::Semver(v)
        | pnpm_lockfile::VersionPart::RegistryQualified { version: v, .. } => v.to_string(),
        pnpm_lockfile::VersionPart::File(v) | pnpm_lockfile::VersionPart::NonSemver(v) => v.clone(),
    }
}

fn lockfile_peer_ranges(metadata: &pnpm_lockfile::PackageMetadata) -> PeerRanges {
    metadata.peer_dependencies
        .iter()
        .flatten()
        .map(|(peer_name, range)| (peer_name.clone(), range.clone()))
        .collect()
}

fn peer_ranges_of(package: &ResolvedPackage) -> PeerRanges {
    package.peer_dependencies
        .iter()
        .map(|(peer_name, peer)| (peer_name.clone(), peer.version.clone()))
        .collect()
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
