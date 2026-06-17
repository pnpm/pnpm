use crate::LockfileResolution;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Metadata for one resolved package version, as stored in the v9
/// `packages:` map. This is the per-version data that does not vary by
/// peer-dependency context — peer-specific information lives in
/// [`SnapshotEntry`](crate::SnapshotEntry) instead.
///
/// Specification: <https://github.com/pnpm/spec/blob/834f2815cc/lockfile/9.0.md>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMetadata {
    pub resolution: LockfileResolution,

    /// Emitted only for non-registry packages (depPath contains `:`) whose
    /// manifest carries a version and whose resolution isn't a directory —
    /// matching pnpm's `toLockfileDependency`. Registry packages omit it
    /// because the version is already the depPath suffix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub engines: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub libc: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<Vec<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMeta>>,
}

// Some packages declare `libc` as a plain string in `package.json`; pnpm writes
// that string as-is into the lockfile.
fn deserialize_string_or_vec<'de, Value, Deser>(
    deserializer: Deser,
) -> Result<Option<Vec<Value>>, Deser::Error>
where
    Value: Deserialize<'de>,
    Deser: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec<Value> {
        String(Value),
        Vec(Vec<Value>),
    }

    let opt = Option::<StringOrVec<Value>>::deserialize(deserializer)?;
    Ok(opt.map(|value| match value {
        StringOrVec::String(item) => vec![item],
        StringOrVec::Vec(items) => items,
    }))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerDependencyMeta {
    pub optional: bool,
}

#[cfg(test)]
mod tests;
