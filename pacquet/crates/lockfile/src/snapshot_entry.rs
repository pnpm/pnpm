use crate::{PkgName, SnapshotDepRef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

/// Per-instance snapshot information stored in the v9 `snapshots:` map.
///
/// An entry describes the wiring of one concrete installation of a package:
/// which versions its dependencies were resolved to, plus any optional /
/// transitive-peer metadata needed to recreate the install.
///
/// Specification: <https://github.com/pnpm/spec/blob/834f2815cc/lockfile/9.0.md>
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
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
    /// pnpm's resolver at install time and written verbatim into
    /// `snapshots[<key>].optional`. Pacquet trusts the precomputed
    /// flag rather than re-deriving from the importer graph,
    /// matching upstream's `lockfileToDepGraph` at
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-builder/src/lockfileToDepGraph.ts#L315>.
    ///
    /// `BuildModules` consults this flag to decide whether a failed
    /// build should be swallowed and reported via
    /// `pnpm:skipped-optional-dependency` (mirrors
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L218-L240>).
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if is called as f(&field)"
)]
fn is_false(value: &bool) -> bool {
    !*value
}
