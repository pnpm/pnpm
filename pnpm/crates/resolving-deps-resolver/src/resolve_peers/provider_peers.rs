//! The peers a package declares, checked against the versions an
//! importer provides — so an optional peer provider taken from elsewhere
//! in the graph is not hoisted where its own peers would conflict.

use super::context;
use crate::{
    parent_pkg_aliases::declared_peer_dependencies, resolve_dependency_tree::WorkspaceTreeCtx,
    resolved_tree::ResolvedPackage,
};
use pnpm_resolving_resolver_base::get_peer_version_range;
use rustc_hash::FxHashMap as HashMap;
use std::cell::{OnceCell, RefCell};

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

impl CandidatePeer {
    /// The peers `manifest` declares.
    pub(crate) fn from_manifest(manifest: &serde_json::Value) -> Vec<CandidatePeer> {
        declared_peer_dependencies(manifest)
            .into_iter()
            .map(|(name, peer)| CandidatePeer {
                name,
                range: peer.version,
                optional: peer.optional,
            })
            .collect()
    }
}

/// Looks up the peers an optional peer candidate declares. A candidate is
/// found by name and version, since a package resolved from a named
/// registry has a different id. One seeded only from the wanted lockfile
/// has no resolved package yet, so the lockfile describes its peers
/// instead. A package resolved only for the check, without being added to
/// the tree, is described by its own manifest first. The name-and-version
/// index is built on the first miss and reused for the rest of the
/// selection.
pub(crate) struct CandidatePeers<'workspace> {
    workspace: &'workspace WorkspaceTreeCtx,
    detached: RefCell<HashMap<String, Vec<CandidatePeer>>>,
    by_name_version: OnceCell<HashMap<String, Vec<CandidatePeer>>>,
}

impl<'workspace> CandidatePeers<'workspace> {
    pub(crate) fn new(workspace: &'workspace WorkspaceTreeCtx) -> Self {
        CandidatePeers { workspace, detached: RefCell::default(), by_name_version: OnceCell::new() }
    }

    /// Records the peers of `pkg_id`, a `name@version` resolved without
    /// being added to the tree.
    pub(crate) fn insert_detached(&self, pkg_id: String, peers: Vec<CandidatePeer>) {
        self.detached.borrow_mut().insert(pkg_id, peers);
    }

    /// The peers `name@version` declares, or `None` when nothing describes
    /// it.
    pub(crate) fn get(&self, name: &str, version: &str) -> Option<Vec<CandidatePeer>> {
        let pkg_id = format!("{name}@{version}");
        if let Some(peers) = self.detached.borrow().get(&pkg_id) {
            return Some(peers.clone());
        }
        if let Some(peers) = self.workspace.inspect_package(&pkg_id, peers_of) {
            return Some(peers);
        }
        self.by_name_version()
            .get(&pkg_id)
            .cloned()
    }

    /// Whether [`Self::get`] knows the peers of `name@version`.
    pub(crate) fn contains(&self, name: &str, version: &str) -> bool {
        let pkg_id = format!("{name}@{version}");
        self.detached.borrow().contains_key(&pkg_id)
            || self.workspace
                .inspect_package(&pkg_id, |_| ())
                .is_some()
            || self.by_name_version().contains_key(&pkg_id)
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

/// Picks the `(name, version)` pairs to hoist for the missing peers each
/// package of a wave declares, or `None` when a required one cannot be
/// hoisted because its ranges conflict.
pub(crate) type RequiredPeerVersionsPicker<'a> =
    dyn Fn(&[Vec<CandidatePeer>]) -> Option<Vec<(String, String)>> + 'a;

/// Whether hoisting the optional peer candidate `name@version` to an
/// importer leaves every peer it brings in satisfied. The candidate
/// resolves its peers from `provided_versions`. The required peers missing
/// there are hoisted together, one wave at a time, at the `(name, version)`
/// pairs `pick_required_peer_versions` returns for the missing required
/// peers each package of the wave declares. They resolve their own peers
/// the same way. A wave that rejects a version already known stops the
/// check before the next wave is picked.
pub(crate) fn accepts_optional_peer_candidate(
    candidate_peers: &CandidatePeers<'_>,
    pick_required_peer_versions: &RequiredPeerVersionsPicker<'_>,
    provided_versions: &HashMap<String, String>,
    (name, version): (&str, &str),
) -> bool {
    let mut hoisted = HashMap::default();
    hoisted.insert(name.to_string(), version.to_string());
    let mut peers_of_hoisted = Vec::new();
    let mut wave = vec![(name.to_string(), version.to_string())];
    while !wave.is_empty() {
        let peers_of_wave: Vec<Vec<CandidatePeer>> = wave
            .iter()
            .filter_map(|(name, version)| candidate_peers.get(name, version))
            .collect();
        let version_of = |name: &str| {
            hoisted
                .get(name)
                .or_else(|| provided_versions.get(name))
        };
        if !peers_of_wave.iter().all(|peers| peers_accept_versions(peers, &version_of)) {
            return false;
        }
        let missing_of_wave: Vec<Vec<CandidatePeer>> = peers_of_wave
            .iter()
            .map(|peers| missing_peers(peers, &version_of))
            .collect();
        let Some(picked) = pick_required_peer_versions(&missing_of_wave) else { return false };
        wave = picked;
        hoisted.extend(wave.iter().cloned());
        peers_of_hoisted.extend(peers_of_wave);
    }
    let version_of = |name: &str| {
        hoisted
            .get(name)
            .or_else(|| provided_versions.get(name))
    };
    peers_of_hoisted.iter().all(|peers| peers_accept_versions(peers, &version_of))
}

fn missing_peers<'a>(
    peers: &[CandidatePeer],
    version_of: &dyn Fn(&str) -> Option<&'a String>,
) -> Vec<CandidatePeer> {
    peers
        .iter()
        .filter(|peer| version_of(&peer.name).is_none())
        .cloned()
        .collect()
}

/// Whether each of `peers` accepts the version that `version_of` maps it
/// to. A peer with no version passes.
fn peers_accept_versions<'a>(
    peers: &[CandidatePeer],
    version_of: &dyn Fn(&str) -> Option<&'a String>,
) -> bool {
    peers
        .iter()
        .all(|peer| {
            version_of(&peer.name)
                .is_none_or(|version| {
                    context::satisfies_with_prereleases(
                        version,
                        &get_peer_version_range(&peer.range),
                    )
                })
        })
}
