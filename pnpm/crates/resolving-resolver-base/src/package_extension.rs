use serde::Deserialize;
use std::collections::BTreeMap;

/// One `packageExtensions` entry: a subset of a manifest's dependency
/// groups, merged onto every matching manifest at install time. The
/// fields are `dependencies`, `optionalDependencies`,
/// `peerDependencies`, and `peerDependenciesMeta`.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackageExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_dependencies: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies_meta: Option<BTreeMap<String, PeerDependencyMeta>>,
}

/// `peerDependenciesMeta` entry shape: a single `optional` flag.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PeerDependencyMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
}
