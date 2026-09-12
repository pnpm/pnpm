use super::{HashMap, IndexMap, ReporterOptions, napi};

/// One importer: an absolute directory plus its in-memory manifest.
#[napi(object)]
pub struct NodeApiProject {
    pub root_dir: String,
    pub manifest: serde_json::Value,
    /// Manifest used when this project is resolved as a *dependency* of
    /// another importer (an injected workspace instance) instead of
    /// `manifest`. Lets an embedder pre-transform its importer manifests
    /// (e.g. strip workspace-sibling deps it links itself) while dependency
    /// instances keep the raw graph — without a `readPackage` hook round
    /// trip. Omit it when both views are the same.
    pub dependency_manifest: Option<serde_json::Value>,
}

/// Options for [`install`](fn@crate::install). Mirrors [`InstallOptions`] in `index.d.ts`; only the
/// fields the engine consumes today are read, the rest are accepted and
/// ignored so the contract stays forward-compatible.
#[napi(object)]
#[derive(Default)]
pub struct InstallOptions {
    pub dir: String,
    pub projects: Vec<NodeApiProject>,
    pub store_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub registries: Option<HashMap<String, String>>,
    pub auth_config: Option<HashMap<String, String>>,
    pub proxy_config: Option<ProxyConfigInput>,
    pub network_config: Option<NetworkConfigInput>,
    pub node_linker: Option<String>,
    /// `linkWorkspacePackages` — `true` / `false` / `"deep"`. When enabled, a
    /// bare-semver dependency may resolve to a workspace package by name (not
    /// only `workspace:`-prefixed ranges).
    pub link_workspace_packages: Option<serde_json::Value>,
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
    pub external_dependencies: Option<Vec<String>>,
    /// `IndexMap` so the JS object's key order survives into
    /// `pnpm-lock.yaml#overrides` — a `HashMap` here reordered the
    /// recorded block at random on every install.
    pub overrides: Option<IndexMap<String, String>>,
    pub package_import_method: Option<String>,
    pub auto_install_peers: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    pub lockfile_only: Option<bool>,
    pub frozen_lockfile: Option<bool>,
    pub prefer_frozen_lockfile: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub offline: Option<bool>,
    pub virtual_store_dir_max_length: Option<u32>,
    /// Whether to use the shared global virtual store for dependency slots.
    pub enable_global_virtual_store: Option<bool>,
    /// Overrides the global virtual store directory.
    pub global_virtual_store_dir: Option<String>,
    /// Manifest fields to add to packages selected by name or version range.
    pub package_extensions: Option<IndexMap<String, PackageExtensionInput>>,
    /// Patch paths keyed by package selector. Relative paths resolve from `dir`.
    pub patched_dependencies: Option<IndexMap<String, String>>,
    /// Warn instead of failing with `ERR_PNPM_UNUSED_PATCH` when a
    /// `patchedDependencies` entry matches no installed package. Lets an
    /// embedder ship a patch keyed to a version range that only some
    /// workspaces resolve.
    pub allow_unused_patches: Option<bool>,
    pub peers_suffix_max_length: Option<u32>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub inject_workspace_packages: Option<bool>,
    pub hoist_workspace_packages: Option<bool>,
    pub enable_modules_dir: Option<bool>,
    /// Install from the lockfile alone, ignoring the project manifests —
    /// pnpm's `pnpm fetch` semantics: the frozen path, no post-import
    /// linking, and no project lifecycle scripts.
    pub ignore_package_manifest: Option<bool>,
    pub node_version: Option<String>,
    pub engine_strict: Option<bool>,
    pub minimum_release_age: Option<u32>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub never_built_dependencies: Option<Vec<String>>,
    pub update: Option<bool>,
    pub depth: Option<u32>,
    pub include_optional_deps: Option<bool>,
    pub ignore_scripts: Option<bool>,
    /// Trust lockfile resolutions without verifying them against current
    /// registry metadata.
    pub trust_lockfile: Option<bool>,
    pub network_concurrency: Option<u32>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u32>,
    pub fetch_retry_maxtimeout: Option<u32>,
    pub fetch_timeout: Option<u32>,
    /// Slow metadata-request threshold in milliseconds. When set, this takes
    /// precedence over the same field in `networkConfig`.
    pub fetch_warn_timeout_ms: Option<u32>,
    /// Minimum average tarball speed in KiB/s. When set, this takes precedence
    /// over the same field in `networkConfig`.
    pub fetch_min_speed_ki_bps: Option<u32>,
    pub user_agent: Option<String>,
    /// Fail the install with `ERR_PNPM_IGNORED_BUILDS` when a dependency build
    /// script is blocked. Defaults to `false` — the install instead reports the
    /// blocked packages in `depsRequiringBuild`, matching how embedders (Bit)
    /// gate builds themselves.
    pub strict_dep_builds: Option<bool>,
    /// Return the dep paths of every package whose files carry install
    /// scripts, regardless of the allow-build policy, in
    /// `depsRequiringBuild`. The list is computed only when a fresh
    /// resolve materializes `node_modules`; an install served from the
    /// frozen-lockfile path (or `lockfileOnly`) leaves
    /// `depsRequiringBuild` undefined so the embedder keeps its
    /// previously recorded list.
    pub return_list_of_deps_requiring_build: Option<bool>,
    /// Per-package build-script allow-list: `name -> allowed`.
    pub allow_builds: Option<HashMap<String, bool>>,
    /// Allow every dependency's build scripts to run.
    pub dangerously_allow_all_builds: Option<bool>,
    /// `peerDependencyRules` — how peer-dependency mismatches are treated.
    pub peer_dependency_rules: Option<PeerDependencyRulesInput>,
    /// Pre-computed `Authorization` header values keyed by nerf-darted registry
    /// URI (`//host/path/`), plus `""` for the default registry — which the
    /// engine pins to the `registry` / `registries.default` passed alongside
    /// it, never to a registry the project's own `.npmrc` names.
    pub auth_header_by_uri: Option<HashMap<String, String>>,
    /// The pnpm home directory the default store location is resolved under
    /// when no `storeDir` is configured (`<pnpmHomeDir>/store`, with pnpm's
    /// same-volume fallback).
    pub pnpm_home_dir: Option<String>,
    /// Render pnpm's own terminal output for this call. Omitted, the call
    /// prints nothing and the embedder renders the `onLog` event stream
    /// itself (or not at all).
    pub reporter: Option<ReporterOptions>,
}

/// Options for [`get_peer_dependency_issues`](crate::get_peer_dependency_issues). Mirrors the TypeScript
/// declaration in `index.d.ts`.
#[napi(object)]
pub struct PeerIssuesOptions {
    pub dir: String,
    pub projects: Vec<NodeApiProject>,
    pub store_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub registries: Option<HashMap<String, String>>,
    pub auth_header_by_uri: Option<HashMap<String, String>>,
    pub proxy_config: Option<ProxyConfigInput>,
    pub network_config: Option<NetworkConfigInput>,
    pub overrides: Option<IndexMap<String, String>>,
    // napi narrows JavaScript numbers to `u32` without rejecting overflow, so
    // these enter as numbers and are checked before conversion.
    pub peers_suffix_max_length: Option<f64>,
    pub virtual_store_dir_max_length: Option<f64>,
    pub auto_install_peers: Option<bool>,
}

#[napi(object)]
#[expect(clippy::struct_field_names, reason = "fields mirror the JavaScript proxyConfig contract")]
pub struct ProxyConfigInput {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<serde_json::Value>,
}

#[napi(object)]
pub struct NetworkConfigInput {
    pub ca: Option<serde_json::Value>,
    pub cert: Option<serde_json::Value>,
    pub key: Option<String>,
    pub local_address: Option<String>,
    pub strict_ssl: Option<bool>,
    pub max_sockets: Option<u32>,
    pub network_concurrency: Option<u32>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u32>,
    pub fetch_retry_maxtimeout: Option<u32>,
    pub fetch_timeout: Option<u32>,
    /// Slow metadata-request threshold in milliseconds. Used when the
    /// corresponding top-level install option is omitted.
    pub fetch_warn_timeout_ms: Option<u32>,
    /// Minimum average tarball speed in KiB/s. Used when the corresponding
    /// top-level install option is omitted.
    pub fetch_min_speed_ki_bps: Option<u32>,
    pub user_agent: Option<String>,
}

/// Manifest fields to add to a matching package.
#[napi(object)]
pub struct PackageExtensionInput {
    pub dependencies: Option<HashMap<String, String>>,
    pub optional_dependencies: Option<HashMap<String, String>>,
    pub peer_dependencies: Option<HashMap<String, String>>,
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMetaInput>>,
}

/// Metadata for a peer dependency.
#[napi(object)]
pub struct PeerDependencyMetaInput {
    pub optional: Option<bool>,
}

/// `peerDependencyRules` input. Mirrors `PeerDependencyRules` in `index.d.ts`.
#[napi(object)]
pub struct PeerDependencyRulesInput {
    pub ignore_missing: Option<Vec<String>>,
    pub allow_any: Option<Vec<String>>,
    pub allowed_versions: Option<HashMap<String, String>>,
}
