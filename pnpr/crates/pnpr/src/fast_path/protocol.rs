//! Wire types for the pnpr fast-path endpoints, matching the pnpm-agent
//! TypeScript client's request shapes.

use std::collections::BTreeMap;

use serde::Deserialize;

pub type DepMap = BTreeMap<String, String>;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequestProject {
    #[serde(default)]
    pub dependencies: DepMap,
    #[serde(default)]
    pub dev_dependencies: DepMap,
}

/// Body of `POST /v1/install`. The registry fields carry the *client's*
/// resolution configuration so the server resolves against the same
/// registries the client would. Unknown fields (`lockfile`,
/// `node_version`, `os`, `arch`) are accepted and ignored so
/// older/newer clients still parse.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    #[serde(default)]
    pub dependencies: Option<DepMap>,
    #[serde(default)]
    pub dev_dependencies: Option<DepMap>,
    #[serde(default)]
    pub projects: Option<Vec<InstallRequestProject>>,
    #[serde(default)]
    pub store_integrities: Vec<String>,
    /// The client's default registry. Falls back to npmjs when absent.
    #[serde(default)]
    pub registry: Option<String>,
    /// The client's named-registry aliases (`pnpm-workspace.yaml`
    /// `namedRegistries`).
    #[serde(default)]
    pub named_registries: BTreeMap<String, String>,
    /// The client's `overrides` (selector -> spec), applied at resolve
    /// time. Kept as raw JSON; reconstructed into pacquet's override map
    /// server-side.
    #[serde(default)]
    pub overrides: Option<serde_json::Value>,
    /// Minimum package age (minutes) before a version is acceptable.
    #[serde(default)]
    pub minimum_release_age: Option<u64>,
}

/// The dependency maps for a single project, normalized across the
/// legacy single-project body and the `projects` array.
pub struct ProjectDeps {
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
}

impl InstallRequest {
    /// Number of projects in the request; the legacy single-project
    /// body counts as one.
    pub fn project_count(&self) -> usize {
        self.projects.as_ref().map_or(1, Vec::len)
    }

    pub fn single_project(&self) -> ProjectDeps {
        if let Some(project) = self.projects.as_ref().and_then(|projects| projects.first()) {
            return ProjectDeps {
                dependencies: project.dependencies.clone(),
                dev_dependencies: project.dev_dependencies.clone(),
            };
        }
        ProjectDeps {
            dependencies: self.dependencies.clone().unwrap_or_default(),
            dev_dependencies: self.dev_dependencies.clone().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FilesRequest {
    pub digests: Vec<FileDigest>,
}

#[derive(Debug, Deserialize)]
pub struct FileDigest {
    pub digest: String,
    #[serde(default)]
    pub executable: bool,
}

/// A valid sha512 digest is 128 lowercase hex chars. The all-zero
/// digest is rejected because it collides with the 64-byte end-of-
/// stream marker in the `/v1/files` binary framing.
pub fn is_valid_sha512_hex(digest: &str) -> bool {
    digest.len() == 128
        && digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && digest.bytes().any(|byte| byte != b'0')
}
