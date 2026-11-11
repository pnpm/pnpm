//! How a snapshot of the `snapshots` list is written and read: its bindings
//! go in one `peerDependencies` map, the package's own as `{version}` and
//! those a dependency below needs as `{version, below: true}`, while the
//! snapshot in memory keeps them in two maps.

use super::{
    NextRef, NextSnapshotEntry,
    package::{BindingOut, Bindings, PeerEntry, SortedMap},
};
use crate::PkgName;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};

/// A snapshot as read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SnapshotWire {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    dependencies: Option<HashMap<PkgName, NextRef>>,
    #[serde(default)]
    optional_dependencies: Option<HashMap<PkgName, NextRef>>,
    #[serde(default)]
    peer_dependencies: Option<HashMap<PkgName, PeerEntry>>,
    #[serde(default)]
    unbound_peer_dependencies: Option<Vec<String>>,
    #[serde(default)]
    optional: bool,
}

impl From<SnapshotWire> for NextSnapshotEntry {
    fn from(wire: SnapshotWire) -> Self {
        let bindings = Bindings::of_entries(wire.peer_dependencies.unwrap_or_default());
        NextSnapshotEntry {
            id: wire.id,
            dependencies: wire.dependencies,
            optional_dependencies: wire.optional_dependencies,
            peer_dependencies: bindings.own,
            transitive_peer_dependencies: bindings.below,
            unbound_peer_dependencies: wire.unbound_peer_dependencies,
            optional: wire.optional,
        }
    }
}

/// A snapshot as written, borrowing what it can.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotWireRef<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dependencies: Option<SortedMap<'a, PkgName, NextRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_dependencies: Option<SortedMap<'a, PkgName, NextRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_dependencies: Option<BTreeMap<String, BindingOut<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unbound_peer_dependencies: &'a Option<Vec<String>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    optional: bool,
}

impl Serialize for NextSnapshotEntry {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        SnapshotWireRef {
            id: &self.id,
            dependencies: self.dependencies.as_ref().map(SortedMap),
            optional_dependencies: self.optional_dependencies.as_ref().map(SortedMap),
            peer_dependencies: Bindings::entries(
                self.peer_dependencies.as_ref(),
                self.transitive_peer_dependencies.as_ref(),
            ),
            unbound_peer_dependencies: &self.unbound_peer_dependencies,
            optional: self.optional,
        }
        .serialize(serializer)
    }
}
