use crate::{PkgName, SnapshotDepRef};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
mod tests;

/// Per-instance snapshot information stored in the v9 `snapshots:` map.
///
/// An entry describes the wiring of one concrete installation of a package:
/// which versions its dependencies were resolved to, plus any optional /
/// transitive-peer metadata needed to recreate the install.
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
    pub artifact_pins: Option<ArtifactPins>,

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

pub type ArtifactPins = BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>;

impl SnapshotEntry {
    pub fn record_artifact_pin(
        &mut self,
        input_key: String,
        owner: String,
        platform_fingerprint: String,
        envelope_digest: String,
    ) -> bool {
        let previous = self
            .artifact_pins
            .as_ref()
            .and_then(|pins| pins.get(&input_key))
            .and_then(|owners| owners.get(&owner))
            .and_then(|platforms| platforms.get(&platform_fingerprint));
        if previous == Some(&envelope_digest) {
            return false;
        }
        let pins = self.artifact_pins.get_or_insert_default();
        let owners = pins.entry(input_key).or_default();
        owners.entry(owner).or_default().insert(platform_fingerprint, envelope_digest);
        true
    }

    pub fn clear_artifact_pins(&mut self) -> bool {
        self.artifact_pins.take().is_some()
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if is called as f(&field)"
)]
fn is_false(value: &bool) -> bool {
    !*value
}
