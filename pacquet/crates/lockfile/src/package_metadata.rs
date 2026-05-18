use crate::LockfileResolution;
use serde::{Deserialize, Serialize};
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMeta>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerDependencyMeta {
    pub optional: bool,
}
