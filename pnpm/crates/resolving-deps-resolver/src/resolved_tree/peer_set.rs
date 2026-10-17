use super::PeerDep;
use std::collections::BTreeMap;

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
