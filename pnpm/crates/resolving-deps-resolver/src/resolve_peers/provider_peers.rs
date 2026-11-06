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
    (package.name().to_string(), package.version().to_string())
}

/// One peer a package declares.
#[derive(Debug, Clone)]
pub(crate) struct CandidatePeer {
    pub(crate) name: String,
    pub(crate) range: String,
    pub(crate) optional: bool,
}

/// Looks up the peers an optional peer candidate declares. A candidate is
/// found by name and version, since a package resolved from a named
/// registry has a different id. One seeded only from the wanted lockfile
/// has no resolved package yet, so the lockfile describes its peers
/// instead. The name-and-version index is built on the first miss and
/// reused for the rest of the selection.
pub(crate) struct CandidatePeers<'workspace> {
    workspace: &'workspace WorkspaceTreeCtx,
    by_name_version: OnceCell<HashMap<String, Vec<CandidatePeer>>>,
}

impl<'workspace> CandidatePeers<'workspace> {
    pub(crate) fn new(workspace: &'workspace WorkspaceTreeCtx) -> Self {
        CandidatePeers { workspace, by_name_version: OnceCell::new() }
    }

    /// The peers `name@version` declares, or `None` when neither the
    /// resolved packages nor the wanted lockfile know it.
    pub(crate) fn get(&self, name: &str, version: &str) -> Option<Vec<CandidatePeer>> {
        let pkg_id = format!("{name}@{version}");
        if let Some(peers) = self.workspace.inspect_package(&pkg_id, peers_of) {
            return Some(peers);
        }
        self.by_name_version()
            .get(&pkg_id)
            .cloned()
    }

    fn by_name_version(&self) -> &HashMap<String, Vec<CandidatePeer>> {
        self.by_name_version.get_or_init(|| {
            let mut index = HashMap::default();
            if let Some(packages) = self.workspace
                .wanted_lockfile()
                .and_then(|lockfile| lockfile.packages.as_ref())
            {
                for (key, metadata) in packages {
                    let version = lockfile_entry_version(key, metadata);
                    index.insert(format!("{}@{version}", key.name), lockfile_peers(metadata));
                }
            }
            self.workspace.for_each_package(|package| {
                index.insert(
                    format!("{}@{}", package.name(), package.version()),
                    peers_of(package),
                );
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

fn lockfile_peers(metadata: &pnpm_lockfile::PackageMetadata) -> Vec<CandidatePeer> {
    metadata.peer_dependencies
        .iter()
        .flatten()
        .map(|(peer_name, range)| CandidatePeer {
            name: peer_name.clone(),
            range: range.clone(),
            optional: metadata.peer_dependencies_meta
                .as_ref()
                .and_then(|meta| meta.get(peer_name))
                .is_some_and(|meta| meta.optional),
        })
        .collect()
}

fn peers_of(package: &ResolvedPackage) -> Vec<CandidatePeer> {
    package.peer_set
        .declared()
        .iter()
        .map(|(peer_name, peer)| CandidatePeer {
            name: peer_name.clone(),
            range: peer.version.clone(),
            optional: peer.optional,
        })
        .collect()
}

/// Whether hoisting the optional peer candidate `name@version` to an
/// importer leaves every peer it brings in satisfied. The candidate
/// resolves its peers from `provided_versions`. Each required peer missing
/// there is hoisted at the version `required_peer_version` returns, and
/// resolves its own peers the same way.
pub(crate) fn accepts_optional_peer_candidate(
    candidate_peers: &CandidatePeers<'_>,
    required_peer_version: &dyn Fn(&str, &str) -> Option<String>,
    provided_versions: &HashMap<String, String>,
    (name, version): (&str, &str),
) -> bool {
    let mut provided = provided_versions.clone();
    provided.insert(name.to_string(), version.to_string());
    let mut peers_of_hoisted = Vec::new();
    let mut pending = vec![(name.to_string(), version.to_string())];
    while let Some((name, version)) = pending.pop() {
        let Some(peers) = candidate_peers.get(&name, &version) else { continue };
        pending.extend(hoist_missing_required_peers(&peers, required_peer_version, &mut provided));
        peers_of_hoisted.push(peers);
    }
    peers_of_hoisted.iter().all(|peers| peers_accept_provided_versions(peers, &provided))
}

/// Adds to `provided` each required peer of `peers` it misses and that is
/// hoisted, and returns them as `(name, version)`.
fn hoist_missing_required_peers(
    peers: &[CandidatePeer],
    required_peer_version: &dyn Fn(&str, &str) -> Option<String>,
    provided: &mut HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut hoisted = Vec::new();
    for peer in peers {
        if peer.optional || provided.contains_key(&peer.name) {
            continue;
        }
        let Some(version) = required_peer_version(&peer.name, &peer.range) else { continue };
        provided.insert(peer.name.clone(), version.clone());
        hoisted.push((peer.name.clone(), version));
    }
    hoisted
}

/// Whether each of `peers` accepts the version that `provided_versions`
/// maps it to. A peer with no entry passes.
fn peers_accept_provided_versions(
    peers: &[CandidatePeer],
    provided_versions: &HashMap<String, String>,
) -> bool {
    peers
        .iter()
        .all(|peer| {
            provided_versions
                .get(&peer.name)
                .is_none_or(|version| {
                    context::satisfies_with_prereleases(
                        version,
                        &get_peer_version_range(&peer.range),
                    )
                })
        })
}
