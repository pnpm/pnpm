use crate::{PkgName, RecordedPeerBindings, SnapshotDepRef};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
mod tests;

/// Per-instance snapshot information stored in the v9 `snapshots:` map.
///
/// An entry describes the wiring of one concrete installation of a package:
/// which versions its dependencies were resolved to, plus any optional /
/// transitive-peer metadata needed to recreate the install.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    /// In-memory bindings for the exact key restored from recorded peers.
    /// Not part of the serialized entry or its equality comparison.
    #[serde(skip)]
    pub recorded_peer_bindings: Option<Arc<RecordedPeerBindings>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dependencies: Option<HashMap<PkgName, SnapshotDepRef>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub optional_dependencies: Option<HashMap<PkgName, SnapshotDepRef>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transitive_peer_dependencies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patched: Option<bool>,

    /// `true` when every path from any importer to this package
    /// goes through an `optionalDependencies` edge — folded by
    /// the resolver at install time and written verbatim into
    /// `snapshots[<key>].optional`. Pacquet trusts the precomputed
    /// flag rather than re-deriving from the importer graph.
    ///
    /// `BuildModules` consults this flag to decide whether a failed
    /// build should be swallowed and reported via
    /// `pnpm:skipped-optional-dependency`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
}

impl PartialEq for SnapshotEntry {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            id,
            dependencies,
            optional_dependencies,
            transitive_peer_dependencies,
            patched,
            optional,
            recorded_peer_bindings: _,
        } = self;
        id == &other.id
            && dependencies == &other.dependencies
            && optional_dependencies == &other.optional_dependencies
            && transitive_peer_dependencies == &other.transitive_peer_dependencies
            && patched == &other.patched
            && optional == &other.optional
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if is called as f(&field)"
)]
fn is_false(value: &bool) -> bool {
    !*value
}
