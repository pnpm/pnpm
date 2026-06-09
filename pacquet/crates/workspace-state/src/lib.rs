//! Read and write pnpm's `node_modules/.pnpm-workspace-state-v1.json`.
//!
//! Mirrors pnpm v11's `@pnpm/workspace.state` package. See upstream
//! <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/index.ts>.
//!
//! The file records what an install actually used (project list,
//! resolved settings, pnpmfiles, ...) so the next `pnpm run` invocation
//! can decide whether `node_modules` is still up to date without
//! re-resolving anything. Mirroring the on-disk shape byte-for-byte
//! lets pnpm read state written by pacquet — that's what closes the
//! gap that forced
//! [`verify-deps-before-run=false`](https://github.com/pnpm/pnpm/commit/7ff112bac6).

use derive_more::{Display, Error};
use indexmap::IndexMap;
use pacquet_diagnostics::miette::{self, Diagnostic};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

/// Basename of the workspace-state file, written inside `node_modules/`.
///
/// Matches upstream's filename at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/filePath.ts>.
pub const WORKSPACE_STATE_FILENAME: &str = ".pnpm-workspace-state-v1.json";

/// `<workspace_dir>/node_modules/.pnpm-workspace-state-v1.json`. Same
/// resolution as upstream's [`getFilePath`](https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/filePath.ts).
#[must_use]
pub fn get_file_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("node_modules").join(WORKSPACE_STATE_FILENAME)
}

/// Per-project entry inside [`WorkspaceState::projects`]. Mirrors
/// upstream's `{ name?, version? }` shape at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/types.ts>.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// A single `configDependencies` value. Mirrors pnpm's
/// `VersionWithIntegrity | { tarball?, integrity }` at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/core/types/src/package.ts>.
/// Untagged so it round-trips both shapes verbatim; pnpm compares the
/// recorded value against the live config with a deep, order-independent
/// equality check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigDependency {
    VersionWithIntegrity(String),
    Detailed(ConfigDependencyDetail),
}

/// The `{ tarball?, integrity }` form of a [`ConfigDependency`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDependencyDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tarball: Option<String>,
    pub integrity: String,
}

/// Typed view of `.pnpm-workspace-state-v1.json`.
///
/// Mirrors upstream's [`WorkspaceState`](https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/types.ts).
/// `lastValidatedTimestamp` is JS `Date.now()` — milliseconds since the
/// Unix epoch — so pnpm's freshness checks (`mtime > lastValidated`)
/// stay consistent across the two implementations.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    pub last_validated_timestamp: i64,
    pub projects: BTreeMap<String, ProjectEntry>,
    pub pnpmfiles: Vec<String>,
    pub filtered_install: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dependencies: Option<BTreeMap<String, ConfigDependency>>,
    pub settings: WorkspaceStateSettings,
}

/// Subset of pnpm's `Config` keys that `checkDepsStatus` compares to
/// the live config before allowing the fast-path. Listed at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/types.ts>.
///
/// Every field is `Option` so pacquet can omit settings it does not
/// track yet. pnpm iterates the full `WORKSPACE_STATE_SETTING_KEYS`
/// list and reads an omitted key as `undefined`, so a key pacquet omits
/// stays compatible only while pnpm's resolved value for it is also
/// `undefined`. Match what the install actually used — if pacquet's
/// resolved value differs from pnpm's, pnpm correctly reinstalls.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStateSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_builds: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_install_peers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_direct_deps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_injected_deps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_peer_dependents: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_peers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<bool>,
    /// `None` and `Some(false)` both mean "global virtual store off" —
    /// pnpm omits the key for its `undefined` default and only writes a
    /// concrete value when `--global` (always `true`) or CI (`false`)
    /// forces one. The freshness check coerces the two off-forms before
    /// comparing (see `enable_global_virtual_store_match`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_global_virtual_store: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_links_from_lockfile: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoist_pattern: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoist_workspace_packages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignored_optional_dependencies: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject_workspace_packages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_workspace_packages: Option<serde_json::Value>,
    /// Minutes a published version must age before it may be installed.
    /// pnpm resolves this to a concrete `24 * 60` default, so it must be
    /// recorded for pnpm's all-key freshness check to stay on the fast
    /// path after a pacquet install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_release_age: Option<u64>,
    /// Whether versions whose registry metadata lacks a `time` field
    /// pass the maturity check. pnpm defaults this to `true`, so it is
    /// recorded for the same reason as [`Self::minimum_release_age`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_release_age_ignore_missing_time: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_linker: Option<NodeLinker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_extensions: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patched_dependencies: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers_suffix_max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_workspace_packages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_hoist_pattern: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_package_patterns: Option<Vec<String>>,
}

/// Mirrors pnpm's `nodeLinker: 'hoisted' | 'isolated' | 'pnp'`. Same
/// wire format as [`pacquet_modules_yaml::NodeLinker`](https://github.com/pnpm/pnpm/blob/7ff112bac6/installing/modules-yaml/src/index.ts);
/// duplicated here rather than depending on `pacquet-modules-yaml` so
/// `workspace-state` stays independent of the install pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeLinker {
    Hoisted,
    Isolated,
    Pnp,
}

/// Error returned by [`update_workspace_state`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UpdateWorkspaceStateError {
    #[display("Failed to create directory {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_state::create_dir))]
    CreateDir { path: PathBuf, source: io::Error },

    #[display("Failed to serialize workspace state: {_0}")]
    #[diagnostic(code(pacquet_workspace_state::serialize_json))]
    SerializeJson(serde_json::Error),

    #[display("Failed to write {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_state::write_io))]
    WriteFile { path: PathBuf, source: io::Error },
}

/// Write `state` to `<workspace_dir>/node_modules/.pnpm-workspace-state-v1.json`.
///
/// Writes to a temporary file in the same directory, then atomically
/// renames it into place, so a concurrent reader — pnpm or pacquet —
/// never observes a half-written file. Mirrors upstream's
/// [`updateWorkspaceState`](https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/updateWorkspaceState.ts),
/// which writes through `write-file-atomic` for the same reason
/// ([#12020](https://github.com/pnpm/pnpm/issues/12020)).
///
/// The serialized bytes are `JSON.stringify(state, undefined, 2) + '\n'`:
/// `serde_json`'s pretty printer uses the same 2-space indent and `": "`
/// separator as JS, so the on-disk bytes round-trip cleanly between the
/// two writers.
pub fn update_workspace_state(
    workspace_dir: &Path,
    state: &WorkspaceState,
) -> Result<(), UpdateWorkspaceStateError> {
    let file_path = get_file_path(workspace_dir);
    let parent = file_path.parent().expect("workspace-state path always has a parent");
    fs::create_dir_all(parent).map_err(|source| UpdateWorkspaceStateError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut serialized =
        serde_json::to_string_pretty(state).map_err(UpdateWorkspaceStateError::SerializeJson)?;
    serialized.push('\n');
    let mut temp = NamedTempFile::new_in(parent).map_err(|source| {
        UpdateWorkspaceStateError::WriteFile { path: file_path.clone(), source }
    })?;
    temp.write_all(serialized.as_bytes()).map_err(|source| {
        UpdateWorkspaceStateError::WriteFile { path: file_path.clone(), source }
    })?;
    temp.persist(&file_path).map_err(|error| UpdateWorkspaceStateError::WriteFile {
        path: file_path,
        source: error.error,
    })?;
    Ok(())
}

/// Read the workspace state file at `<workspace_dir>/node_modules/.pnpm-workspace-state-v1.json`.
///
/// Returns `Ok(None)` when the file does not exist, matching upstream's
/// [`loadWorkspaceState`](https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/loadWorkspaceState.ts).
pub fn load_workspace_state(
    workspace_dir: &Path,
) -> Result<Option<WorkspaceState>, LoadWorkspaceStateError> {
    let file_path = get_file_path(workspace_dir);
    let text = match fs::read_to_string(&file_path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(LoadWorkspaceStateError::ReadFile { path: file_path, source });
        }
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| LoadWorkspaceStateError::ParseJson { path: file_path, source })
}

/// Error returned by [`load_workspace_state`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadWorkspaceStateError {
    #[display("Failed to read {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_state::read_io))]
    ReadFile { path: PathBuf, source: io::Error },

    #[display("Failed to parse {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_state::parse_json))]
    ParseJson { path: PathBuf, source: serde_json::Error },
}

/// Wall-clock milliseconds since the Unix epoch, matching JS
/// `Date.now()` and the `lastValidatedTimestamp` value pnpm writes at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/createWorkspaceState.ts>.
///
/// Truncates to `i64` because the JSON field is signed and the year
/// 2038-pre-292277026596 range is the only one that matters.
#[must_use]
pub fn now_millis() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests;
