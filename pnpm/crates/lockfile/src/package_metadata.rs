use crate::LockfileResolution;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, ops::Deref};

/// Metadata for one resolved package version, as stored in the v9
/// `packages:` map. This is the per-version data that does not vary by
/// peer-dependency context — peer-specific information lives in
/// [`SnapshotEntry`](crate::SnapshotEntry) instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMetadata {
    pub resolution: LockfileResolution,

    /// Emitted only for non-registry packages (depPath contains `:`) whose
    /// manifest carries a version and whose resolution isn't a directory.
    /// Registry packages omit it because the version is already the depPath
    /// suffix.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<StringOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<BundledDependencies>,
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

/// What a package bundles inside its own tarball, as pnpm records it: the
/// bundled names, or the boolean form that covers every entry of
/// `dependencies`. Both manifest spellings (`bundledDependencies` and
/// `bundleDependencies`) land here under the `bundledDependencies` key.
///
/// The presence of the field — whatever its shape — is what tells the
/// installer to link the bins the tarball ships in its own `node_modules`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BundledDependencies {
    Boolean(bool),
    Names(Vec<String>),
}

impl BundledDependencies {
    /// Read the declaration off a package manifest. Only a nonempty list or
    /// `true` is recorded; anything else (including `false` and an empty list)
    /// falls through to the legacy `bundleDependencies` spelling and then to
    /// `None`, because a package that bundles nothing must not look like one
    /// that does. Tarball manifests can carry an empty list where registry
    /// metadata omits the field, so recording it would make the lockfile
    /// depend on the manifest source.
    #[must_use]
    pub fn from_manifest(manifest: Option<&serde_json::Value>) -> Option<Self> {
        ["bundledDependencies", "bundleDependencies"].into_iter().find_map(|key| {
            match manifest?.get(key)? {
                serde_json::Value::Array(items) if !items.is_empty() => {
                    Some(BundledDependencies::Names(
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .collect(),
                    ))
                }
                serde_json::Value::Bool(true) => Some(BundledDependencies::Boolean(true)),
                _ => None,
            }
        })
    }
}

/// A package-manifest field that accepts either one string or a list.
///
/// pnpm preserves this distinction in the lockfile, so retaining only the
/// normalized values is insufficient for byte-identical serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    String(String),
    List(Vec<String>),
}

impl Deref for StringOrList {
    type Target = [String];

    fn deref(&self) -> &Self::Target {
        match self {
            StringOrList::String(value) => std::slice::from_ref(value),
            StringOrList::List(values) => values,
        }
    }
}

impl From<Vec<String>> for StringOrList {
    fn from(values: Vec<String>) -> Self {
        StringOrList::List(values)
    }
}

impl FromIterator<String> for StringOrList {
    fn from_iter<Values: IntoIterator<Item = String>>(iter: Values) -> Self {
        StringOrList::List(iter.into_iter().collect())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerDependencyMeta {
    pub optional: bool,
}

#[cfg(test)]
mod tests;
