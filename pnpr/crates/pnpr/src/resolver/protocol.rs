//! Wire types for the pnpr resolver endpoints, matching the
//! `@pnpm/pnpr.client` TypeScript client's request shapes.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{PackageExtension, RegistryDeclaration};
use pnpm_network::AuthHeadersByScope;
use pnpr_registry::Ecosystem;
use serde::{Deserialize, Serialize, de::IgnoredAny};

pub type DepMap = BTreeMap<String, String>;

/// The `ecosystem` field alone, read before the rest of the body so each
/// ecosystem's request is deserialized into its own shape.
///
/// `POST /-/pnpr/v0/resolve` keeps one address for every ecosystem and the
/// body names which one it speaks, so a client needs no second endpoint and
/// no second handshake. An absent field means npm. The ecosystems pnpr
/// *serves* here are a subset of the ones it knows: a request naming one it
/// does not resolve is refused rather than misread.
#[derive(Debug, Default, Deserialize)]
pub struct EcosystemProbe {
    #[serde(default)]
    pub ecosystem: Ecosystem,
}

/// Body of `POST /-/pnpr/v0/resolve` with `"ecosystem": "cargo"`.
/// Refuses unknown fields, like [`ResolveRequest`].
///
/// Cargo resolution uses the client's workspace dependency graph and
/// lockfile preferences. The server resolves them against index files it
/// fetches (and caches) itself.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CargoResolveRequest {
    /// Read by [`EcosystemProbe`] before this type.
    #[serde(rename = "ecosystem", default)]
    pub _ecosystem: IgnoredAny,
    /// `cargo metadata --no-deps --format-version 1` output for the
    /// workspace being resolved.
    pub metadata: String,
    #[serde(default)]
    pub preferred_versions: BTreeMap<String, Vec<String>>,
    /// The sparse index to resolve against. Falls back to the crates.io
    /// sparse index when absent; either way the server fetches it only if
    /// its route policy allows that origin.
    #[serde(default)]
    pub registry: Option<String>,
}

/// Body of `POST /-/pnpr/v0/resolve` with `"ecosystem": "pypi"`.
///
/// A Python resolution is for one interpreter: the same requirements
/// resolve differently against a different marker environment or a
/// different set of wheel tags, so the client's own travel with the
/// request and the lockfile that comes back records them. Refuses unknown
/// fields, like [`ResolveRequest`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PypiResolveRequest {
    /// Read by [`EcosystemProbe`] before this type.
    #[serde(rename = "ecosystem", default)]
    pub _ecosystem: IgnoredAny,
    /// PEP 508 requirement strings, as the project's manifest spells them.
    pub requirements: Vec<String>,
    /// The interpreter the environment is for.
    pub target: pnpm_python_resolver::Target,
    /// The Simple API base URL to resolve against. The server fetches it
    /// only if its route policy allows that origin.
    pub index: String,
    /// The project's own `requires-python`, recorded in the lockfile.
    #[serde(default)]
    pub requires_python: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPublishConfig {
    pub directory: Option<String>,
    pub link_directory: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveRequestProject {
    /// The importer's directory relative to the lockfile dir, in POSIX
    /// form (`.` for the root, `packages/foo` for a workspace member).
    #[serde(default = "root_dir")]
    pub dir: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub publish_config: Option<ProjectPublishConfig>,
    #[serde(default)]
    pub dependencies: DepMap,
    #[serde(default)]
    pub dev_dependencies: DepMap,
    #[serde(default)]
    pub optional_dependencies: DepMap,
    /// Auto-installed as dependencies of the importer when the request
    /// enables `autoInstallPeers`.
    #[serde(default)]
    pub peer_dependencies: DepMap,
}

fn root_dir() -> String {
    ".".to_string()
}

/// Body of `POST /-/pnpr/v0/resolve`. The registry fields carry the *client's*
/// resolution configuration so the server resolves against the same
/// registries the client would, and the policy fields carry the
/// client's verification policy so the server verifies the input
/// `lockfile` under it before resolving.
///
/// Unknown fields are refused with [`UNSUPPORTED_REQUEST_FIELD_CODE`]: a
/// field this server does not know may be a setting that changes what the
/// client would resolve, and dropping it would return a different lockfile.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "pnpr resolution request wire format is flat"
    )
)]
pub struct ResolveRequest {
    /// Read by [`EcosystemProbe`] before this type.
    #[serde(rename = "ecosystem", default)]
    pub _ecosystem: IgnoredAny,
    #[serde(default)]
    pub dependencies: Option<DepMap>,
    #[serde(default)]
    pub dev_dependencies: Option<DepMap>,
    #[serde(default)]
    pub optional_dependencies: Option<DepMap>,
    #[serde(default)]
    pub peer_dependencies: Option<DepMap>,
    #[serde(default)]
    pub projects: Option<Vec<ResolveRequestProject>>,
    /// The client's default registry. Falls back to npmjs when absent.
    #[serde(default)]
    pub registry: Option<String>,
    /// The registries the client declares, keyed by URL, in the shape of
    /// its `registries` setting: the scopes routed to each, the
    /// bare-specifier prefix each answers to, and each one's tarball
    /// layout. The default registry is not among them — it arrives as
    /// [`Self::registry`].
    ///
    /// Declarations only, never the setting's older `<scope>: <url>` shape:
    /// a key is always a URL, which is what lets the boundary checks read one
    /// as a fetch target. A request in the older shape fails to parse.
    #[serde(default)]
    pub registries: IndexMap<String, RegistryDeclaration>,
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
    /// The client's `patchedDependencies`, with local file paths replaced by
    /// their SHA-256 hashes. Resolution uses the hashes in package snapshot
    /// keys; patch application remains client-side.
    #[serde(default)]
    pub patched_dependencies: Option<IndexMap<String, String>>,
    /// The client's `packageExtensions`, applied to dependency manifests
    /// during server-side resolution.
    #[serde(default)]
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,
    /// Whether configured patches that match no resolved package are allowed.
    #[serde(default)]
    pub allow_unused_patches: bool,
    /// The client's workspace catalogs (`catalog:` / `catalogs:` from
    /// `pnpm-workspace.yaml`), keyed by catalog name with the default
    /// catalog at `"default"`. The reconstructed workspace has no catalog
    /// sections, so these are forwarded and used as the resolution's
    /// catalog set to resolve `catalog:` specifiers in both dependencies
    /// and overrides.
    #[serde(default)]
    pub catalogs: Option<Catalogs>,
    /// The client's current values for the settings that shape the lockfile
    /// this request resolves. `None` is a client that doesn't send them, not
    /// `Some(false)`; `EffectiveResolverSettings` documents what each one
    /// falls back to.
    #[serde(default)]
    pub auto_install_peers: Option<bool>,
    #[serde(default)]
    pub dedupe_peers: Option<bool>,
    #[serde(default)]
    pub exclude_links_from_lockfile: Option<bool>,
    #[serde(default)]
    pub add_missing_peer_types: Option<bool>,
    #[serde(default)]
    pub project_scoped_optional_peers: Option<bool>,
    /// The client's existing on-disk lockfile, when present. Sent both
    /// as the verification target (the server verifies it under the
    /// client's policy before resolving) and as the resolution-reuse
    /// seed. Absent on a true first install (nothing to verify).
    #[serde(default)]
    pub lockfile: Option<pnpm_lockfile::Lockfile>,
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
    /// Refresh registry artifacts while retaining every locked package
    /// version. Omitted by older clients and false for ordinary resolves.
    #[serde(default)]
    pub update_patches: bool,
    /// Re-resolve every edge while preserving compatible locked versions and
    /// regenerating derived lockfile fields.
    #[serde(default)]
    pub fix_lockfile: bool,
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
    /// The client's `resolutionMode`, which decides how a version is
    /// picked: highest satisfying, lowest-satisfying direct, or
    /// time-based (lowest direct, with subdependencies constrained to
    /// what was published by the newest direct dependency). A client
    /// that sends none gets the `highest` default.
    #[serde(default)]
    pub resolution_mode: pnpm_config::ResolutionMode,
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
    pub trust_policy: pnpm_config::TrustPolicy,
    /// Glob patterns opting packages out of the `trustPolicy` check.
    #[serde(default)]
    pub trust_policy_exclude: Option<Vec<String>>,
    /// Minutes after which an old package skips the `trustPolicy`
    /// check.
    #[serde(default)]
    pub trust_policy_ignore_after: Option<u64>,
    /// The [`LockfileFormat`]s the client reads, by their wire names. The
    /// request is refused when none is one the server renders. Absent means
    /// [`LockfileFormat::PnpmLockV9`].
    #[serde(default)]
    pub accepted_lockfile_formats: Option<Vec<String>>,
    /// Sent by the pnpm v11 client. Resolution does not read them: the
    /// lockfile records the packages for every platform and the client's
    /// install picks its own.
    #[serde(rename = "nodeVersion", default)]
    pub _node_version: IgnoredAny,
    #[serde(rename = "os", default)]
    pub _os: IgnoredAny,
    #[serde(rename = "arch", default)]
    pub _arch: IgnoredAny,
}

/// Error code of a request that names a field the server does not know.
pub const UNSUPPORTED_REQUEST_FIELD_CODE: &str = "ERR_PNPR_UNSUPPORTED_REQUEST_FIELD";

/// Error code of a request whose `acceptedLockfileFormats` names no format
/// the server renders.
pub const INCOMPATIBLE_LOCKFILE_FORMAT_CODE: &str = "ERR_PNPR_INCOMPATIBLE_LOCKFILE_FORMAT";

/// The shape of a `done` frame's `lockfile`, named by the frame's
/// `lockfileFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockfileFormat {
    /// A `pnpm-lock.yaml` document with `lockfileVersion: '9.0'`, as JSON.
    PnpmLockV9,
    /// A `Cargo.lock` document, as a TOML string.
    CargoLock,
    /// A `pylock.toml` document, as JSON.
    Pylock,
}

impl LockfileFormat {
    /// The format's name in `acceptedLockfileFormats` and `lockfileFormat`.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            LockfileFormat::PnpmLockV9 => "pnpm-lock/9.0",
            LockfileFormat::CargoLock => "cargo-lock",
            LockfileFormat::Pylock => "pylock",
        }
    }
}

/// The policies the server applied to a resolution, echoed in the `done`
/// frame so the client can verify that requested verification and resolution
/// policies were actually enforced by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the `applied` object of the pnpr `done` frame."
    )
)]
pub struct AppliedPolicies {
    pub auto_install_peers: bool,
    pub dedupe_peers: bool,
    pub exclude_links_from_lockfile: bool,
    pub add_missing_peer_types: bool,
    pub project_scoped_optional_peers: bool,
    pub frozen_lockfile: bool,
    pub resolution_mode: String,
    pub trust_lockfile: bool,
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Vec<String>,
    pub minimum_release_age_ignore_missing_time: bool,
    pub trust_policy: Option<String>,
    pub trust_policy_exclude: Vec<String>,
    pub trust_policy_ignore_after: Option<u64>,
}

impl AppliedPolicies {
    pub fn from_request(request: &ResolveRequest) -> Self {
        let settings = super::config_cache::EffectiveResolverSettings::for_request(request);
        let resolution_mode = match request.resolution_mode {
            pnpm_config::ResolutionMode::Highest => "highest",
            pnpm_config::ResolutionMode::TimeBased => "time-based",
            pnpm_config::ResolutionMode::LowestDirect => "lowest-direct",
        }
        .to_string();
        let trust_policy = match request.trust_policy {
            pnpm_config::TrustPolicy::Off => None,
            pnpm_config::TrustPolicy::NoDowngrade => Some("no-downgrade".to_string()),
        };
        Self {
            minimum_release_age_exclude: request.minimum_release_age_exclude
                .clone()
                .unwrap_or_default(),
            minimum_release_age_ignore_missing_time: request
                .minimum_release_age_ignore_missing_time
                .unwrap_or(true),
            trust_policy_exclude: request.trust_policy_exclude.clone().unwrap_or_default(),
            trust_policy_ignore_after: request.trust_policy_ignore_after,
            trust_lockfile: request.trust_lockfile,
            auto_install_peers: settings.auto_install_peers,
            dedupe_peers: settings.dedupe_peers,
            exclude_links_from_lockfile: settings.exclude_links_from_lockfile,
            add_missing_peer_types: settings.add_missing_peer_types,
            project_scoped_optional_peers: settings.project_scoped_optional_peers,
            frozen_lockfile: request.frozen_lockfile,
            resolution_mode,
            minimum_release_age: request.minimum_release_age,
            trust_policy,
        }
    }
}

/// One project's importer dir and its dependency maps, normalized
/// across the legacy single-project body and the `projects` array.
pub struct ProjectDeps {
    pub dir: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub publish_config: Option<ProjectPublishConfig>,
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
    pub optional_dependencies: DepMap,
    pub peer_dependencies: DepMap,
}

impl ResolveRequest {
    /// Every project to resolve, keyed by importer dir. The legacy
    /// single-project body (top-level `dependencies`/`devDependencies`)
    /// maps to a single root (`.`) importer; an empty/absent `projects`
    /// array falls back to it too.
    pub fn projects_normalized(&self) -> Vec<ProjectDeps> {
        if let Some(projects) = self.projects
            .as_ref()
            .filter(|projects| !projects.is_empty())
        {
            return projects
                .iter()
                .map(|project| ProjectDeps {
                    dir: project.dir.clone(),
                    name: project.name.clone(),
                    version: project.version.clone(),
                    publish_config: project.publish_config.clone(),
                    dependencies: project.dependencies.clone(),
                    dev_dependencies: project.dev_dependencies.clone(),
                    optional_dependencies: project.optional_dependencies.clone(),
                    peer_dependencies: project.peer_dependencies.clone(),
                })
                .collect();
        }
        vec![ProjectDeps {
            dir: root_dir(),
            name: None,
            version: None,
            publish_config: None,
            dependencies: self.dependencies.clone().unwrap_or_default(),
            dev_dependencies: self.dev_dependencies.clone().unwrap_or_default(),
            optional_dependencies: self.optional_dependencies.clone().unwrap_or_default(),
            peer_dependencies: self.peer_dependencies.clone().unwrap_or_default(),
        }]
    }
}
