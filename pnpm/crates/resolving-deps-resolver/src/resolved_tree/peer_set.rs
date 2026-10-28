use super::PeerDep;
use crate::parent_pkg_aliases::{declared_peer_dependencies, own_dependency_names};
use pnpm_lockfile::PeerDependencyMeta;
use std::collections::{BTreeMap, HashMap};

/// The `peerDependencies` (name → range) and `peerDependenciesMeta`
/// (name → `{ optional: true }`) blocks of a `packages:` entry.
pub type LockfilePeerBlocks =
    (Option<HashMap<String, String>>, Option<HashMap<String, PeerDependencyMeta>>);

/// The blocks a `packages:` entry records for the `declared` peers. Every
/// declared peer is recorded, whether or not an install takes it, so the
/// entry is the same in every scope the package resolves in.
#[must_use]
pub fn lockfile_peer_blocks(declared: &BTreeMap<String, PeerDep>) -> LockfilePeerBlocks {
    if declared.is_empty() {
        return (None, None);
    }
    let mut peers = HashMap::new();
    let mut peers_meta = HashMap::new();
    for (name, peer) in declared {
        peers.insert(name.clone(), peer.version.clone());
        if peer.optional {
            peers_meta.insert(name.clone(), PeerDependencyMeta { optional: true });
        }
    }
    (Some(peers), (!peers_meta.is_empty()).then_some(peers_meta))
}

/// A package's peers: every one it declares, and the ones it takes as
/// peers. A declared peer that is also one of the package's own
/// `dependencies` is taken only where its children owner's scope shadows
/// that dependency (see
/// [`peer_shadowed_dependency_names`](crate::peer_shadowed_dependency_names)),
/// so `taken` follows that one decision while `declared` stays the same in
/// every scope.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PeerSet {
    declared: BTreeMap<String, PeerDep>,
    taken: BTreeMap<String, PeerDep>,
}

impl PeerSet {
    /// Classify the declared peers using the children owner's shadowed dependencies.
    #[must_use]
    pub fn from_manifest(manifest: &serde_json::Value, is_shadowed: impl Fn(&str) -> bool) -> Self {
        let own: rustc_hash::FxHashSet<&str> =
            own_dependency_names(manifest, is_shadowed).collect();
        Self::new(declared_peer_dependencies(manifest), |name| own.contains(name))
    }

    /// The `declared` peers, taking each one the package does not supply
    /// itself.
    #[must_use]
    pub fn new(declared: BTreeMap<String, PeerDep>, is_own: impl Fn(&str) -> bool) -> Self {
        let taken = declared
            .iter()
            .filter(|(name, _)| !is_own(name))
            .map(|(name, peer)| (name.clone(), peer.clone()))
            .collect();
        PeerSet { declared, taken }
    }

    /// A package that takes every peer it declares.
    #[must_use]
    pub fn taking_all(declared: BTreeMap<String, PeerDep>) -> Self {
        PeerSet { taken: declared.clone(), declared }
    }

    /// Rewrites the range of every `taken` peer. The `declared` ranges stay
    /// as the manifest writes them.
    pub fn try_map_taken_ranges<Failure>(
        mut self,
        mut map: impl FnMut(&str, String) -> Result<String, Failure>,
    ) -> Result<Self, Failure> {
        for (name, peer) in &mut self.taken {
            peer.version = map(name, std::mem::take(&mut peer.version))?;
        }
        Ok(self)
    }

    /// What the lockfile records for the package.
    #[must_use]
    pub fn declared(&self) -> &BTreeMap<String, PeerDep> {
        &self.declared
    }

    /// What peer resolution binds for the package.
    #[must_use]
    pub fn taken(&self) -> &BTreeMap<String, PeerDep> {
        &self.taken
    }
}
