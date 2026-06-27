//! Wire types for the pnpr resolver endpoints, matching the
//! `@pnpm/pnpr.client` TypeScript client's request shapes.

use std::collections::BTreeMap;

use pacquet_network::AuthHeadersByScope;
use serde::Deserialize;

pub type DepMap = BTreeMap<String, String>;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequestProject {
    /// The importer's directory relative to the lockfile dir, in POSIX
    /// form (`.` for the root, `packages/foo` for a workspace member).
    #[serde(default = "root_dir")]
    pub dir: String,
    #[serde(default)]
    pub dependencies: DepMap,
    #[serde(default)]
    pub dev_dependencies: DepMap,
    #[serde(default)]
    pub optional_dependencies: DepMap,
}

fn root_dir() -> String {
    ".".to_string()
}

/// Body of `POST /-/pnpr/v0/resolve`. The registry fields carry the *client's*
/// resolution configuration so the server resolves against the same
/// registries the client would, and the policy fields carry the
/// client's verification policy so the server verifies the input
/// `lockfile` under it before resolving. Unknown fields (`node_version`,
/// `os`, `arch`) are accepted and ignored so older/newer clients still
/// parse.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequest {
    #[serde(default)]
    pub dependencies: Option<DepMap>,
    #[serde(default)]
    pub dev_dependencies: Option<DepMap>,
    #[serde(default)]
    pub optional_dependencies: Option<DepMap>,
    #[serde(default)]
    pub projects: Option<Vec<ResolveRequestProject>>,
    /// The client's default registry. Falls back to npmjs when absent.
    #[serde(default)]
    pub registry: Option<String>,
    /// The client's named-registry aliases (`pnpm-workspace.yaml`
    /// `namedRegistries`).
    #[serde(default)]
    pub named_registries: BTreeMap<String, String>,
    /// The caller's forwarded upstream credentials so the server resolves
    /// and fetches private content as the caller. Keyed as
    /// `auth_headers[registry_uri][scope]`; the `@` scope stores
    /// registry-wide auth. Distinct from the request's HTTP
    /// `Authorization` header (pnpr identity).
    #[serde(default)]
    pub auth_headers: AuthHeadersByScope,
    /// The client's `overrides` (selector -> spec), applied at resolve
    /// time. Kept as raw JSON; reconstructed into pacquet's override map
    /// server-side.
    #[serde(default)]
    pub overrides: Option<serde_json::Value>,
    /// The client's existing on-disk lockfile, when present. Sent both
    /// as the verification target (the server verifies it under the
    /// client's policy before resolving) and as the resolution-reuse
    /// seed. Absent on a true first install (nothing to verify).
    #[serde(default)]
    pub lockfile: Option<pacquet_lockfile::Lockfile>,
    /// Governs *resolution behavior* only — frozen (use the lockfile
    /// as-is) vs reuse-and-update. Does not affect whether the input
    /// lockfile is verified.
    #[serde(default)]
    pub frozen_lockfile: bool,
    /// `preferFrozenLockfile`. `Some(false)` (from the client's
    /// `--no-prefer-frozen-lockfile`) forces a fresh re-resolve even
    /// when the lockfile is up to date. `None` defaults to reuse.
    #[serde(default)]
    pub prefer_frozen_lockfile: Option<bool>,
    /// `ignoreManifestCheck`: skip the manifest ↔ lockfile freshness
    /// comparison during the frozen resolve.
    #[serde(default)]
    pub ignore_manifest_check: bool,
    /// The client's effective `trustLockfile`. When `true` the client
    /// opted out of lockfile verification, so the server skips the
    /// input-lockfile verify gate (it still reuses the lockfile for
    /// resolution). Mirrors the local path's `--trust-lockfile` /
    /// `trustLockfile` opt-out.
    #[serde(default)]
    pub trust_lockfile: bool,
    /// Minimum package age (minutes) before a version is acceptable.
    #[serde(default)]
    pub minimum_release_age: Option<u64>,
    /// Glob patterns opting packages out of the `minimumReleaseAge`
    /// check.
    #[serde(default)]
    pub minimum_release_age_exclude: Option<Vec<String>>,
    /// Whether to skip the `minimumReleaseAge` check for a version the
    /// registry lists without a publish time. `None` defaults to the
    /// client default (`true`).
    #[serde(default)]
    pub minimum_release_age_ignore_missing_time: Option<bool>,
    /// The client's supply-chain trust policy. Defaults to `off`.
    #[serde(default)]
    pub trust_policy: pacquet_config::TrustPolicy,
    /// Glob patterns opting packages out of the `trustPolicy` check.
    #[serde(default)]
    pub trust_policy_exclude: Option<Vec<String>>,
    /// Minutes after which an old package skips the `trustPolicy`
    /// check.
    #[serde(default)]
    pub trust_policy_ignore_after: Option<u64>,
}

/// One project's importer dir and its dependency maps, normalized
/// across the legacy single-project body and the `projects` array.
pub struct ProjectDeps {
    pub dir: String,
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
    pub optional_dependencies: DepMap,
}

impl ResolveRequest {
    /// Every project to resolve, keyed by importer dir. The legacy
    /// single-project body (top-level `dependencies`/`devDependencies`)
    /// maps to a single root (`.`) importer; an empty/absent `projects`
    /// array falls back to it too.
    pub fn projects_normalized(&self) -> Vec<ProjectDeps> {
        if let Some(projects) = self.projects.as_ref().filter(|projects| !projects.is_empty()) {
            return projects
                .iter()
                .map(|project| ProjectDeps {
                    dir: project.dir.clone(),
                    dependencies: project.dependencies.clone(),
                    dev_dependencies: project.dev_dependencies.clone(),
                    optional_dependencies: project.optional_dependencies.clone(),
                })
                .collect();
        }
        vec![ProjectDeps {
            dir: root_dir(),
            dependencies: self.dependencies.clone().unwrap_or_default(),
            dev_dependencies: self.dev_dependencies.clone().unwrap_or_default(),
            optional_dependencies: self.optional_dependencies.clone().unwrap_or_default(),
        }]
    }
}
