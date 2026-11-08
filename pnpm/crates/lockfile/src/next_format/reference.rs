//! A reference of the next format: a version, `name: version`. Where the
//! referrer's scope does not say which snapshot of that version it links,
//! the reference names the snapshot by the peer bindings that tell the
//! package's snapshots apart: `name: {version, peers: {peer: version}}`.
//! No reference carries a peer or patch group.

use crate::{PkgName, PkgNameVerPeer, SnapshotDepRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The bindings that name a snapshot: the peer's name, and the reference the
/// snapshot binds it to, without groups.
pub type NamedBindings = BTreeMap<String, SnapshotDepRef>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NextRef {
    Plain(SnapshotDepRef),
    Named { version: SnapshotDepRef, peers: NamedBindings },
}

impl NextRef {
    #[must_use]
    pub fn version(&self) -> &SnapshotDepRef {
        match self {
            NextRef::Plain(version) | NextRef::Named { version, .. } => version,
        }
    }

    /// The bindings that name the snapshot, when the reference carries them.
    #[must_use]
    pub fn peers(&self) -> Option<&NamedBindings> {
        match self {
            NextRef::Plain(_) => None,
            NextRef::Named { peers, .. } => Some(peers),
        }
    }

    /// The package key the reference names, without groups.
    #[must_use]
    pub fn resolve(&self, alias: &PkgName) -> Option<PkgNameVerPeer> {
        self.version().resolve(alias)
    }

    #[must_use]
    pub fn as_link_target(&self) -> Option<&str> {
        self.version().as_link_target()
    }
}

/// `reference` with its peer and patch groups dropped.
#[must_use]
pub fn without_groups(reference: &SnapshotDepRef) -> SnapshotDepRef {
    match reference {
        SnapshotDepRef::Plain(version) => SnapshotDepRef::Plain(version.without_peer()),
        SnapshotDepRef::Alias(key) => SnapshotDepRef::Alias(key.without_peer()),
        SnapshotDepRef::Link(path) => SnapshotDepRef::Link(path.clone()),
    }
}

/// Whether `binding`, groups aside, is the reference `wanted` names.
#[must_use]
pub fn names_binding(wanted: &SnapshotDepRef, binding: &SnapshotDepRef) -> bool {
    *wanted == without_groups(binding)
}
