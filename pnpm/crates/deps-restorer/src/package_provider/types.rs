use crate::SkippedSnapshots;
use pnpm_config::Config;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_patching::ExtendedPatchInfo;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub const PROTOCOL_VERSION: u32 = 1;

/// Inputs required to build and dispatch a package provider request.
pub struct PackageProviderInputs<'a> {
    pub package_provider: &'a str,
    pub lockfile_dir: &'a Path,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub skipped: &'a SkippedSnapshots,
    pub patches: Option<&'a HashMap<PackageKey, ExtendedPatchInfo>>,
    pub engine: Option<&'a str>,
    pub config: &'static Config,
}

/// Materialization output returned by the external package provider.
#[derive(Debug, Default)]
pub struct PackageProviderOutput {
    pub paths: HashMap<PackageKey, PathBuf>,
    pub skipped: Vec<PackageKey>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderResolutionSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tarball: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) git: Option<ProviderGitSource>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRequestNode {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(flatten)]
    pub(crate) source: ProviderResolutionSource,
    pub(crate) deps: BTreeMap<String, ProviderRequestDep>,
    pub(crate) engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) optional: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) patch: Option<ProviderPatch>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderGitSource {
    pub(crate) repo: String,
    pub(crate) commit: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRequestDep {
    pub(crate) dep_path: String,
    pub(crate) name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderPatch {
    pub(crate) content: String,
    pub(crate) hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRequest {
    pub(crate) protocol: u32,
    pub(crate) gc_root_dir: String,
    pub(crate) nodes: BTreeMap<String, ProviderRequestNode>,
}

#[derive(Debug)]
pub(crate) struct ProviderRequestBundle {
    pub(crate) request: ProviderRequest,
    pub(crate) key_by_dep_path: HashMap<String, PackageKey>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderResponse {
    pub(crate) protocol: Option<u32>,
    pub(crate) paths: Option<HashMap<String, String>>,
    pub(crate) skipped: Option<Vec<String>>,
}
