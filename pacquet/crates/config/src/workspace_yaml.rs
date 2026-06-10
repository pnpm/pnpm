use crate::{
    CatalogMode, Config, HoistingLimits, LinkWorkspacePackages, NodeLinker, PackageImportMethod,
    ResolutionMode, ScriptsPrependNodePath, TrustPolicy, api::EnvVar, resolve_child_concurrency,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_env_replace::env_replace_lossy;
use pacquet_package_is_installable::SupportedArchitectures;
use pacquet_store_dir::StoreDir;
use pacquet_workspace_state::ConfigDependency;
use pipe_trait::Pipe;
use serde::{Deserialize, Deserializer};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// `serde` helper for fields that need to distinguish "missing key"
/// from "explicit null" in YAML / JSON. Used by `hoist_pattern` and
/// `public_hoist_pattern` so an explicit `hoistPattern: null` in
/// `pnpm-workspace.yaml` propagates as `Some(None)` (= "disable this
/// side"), while a missing key falls through to the field's serde
/// default (`None`, = "leave the config default in place").
///
/// Field shape: `Option<Option<Vec<String>>>`.
/// - `None` — key not present in yaml. `apply_to` skips the field.
/// - `Some(None)` — explicit `null`. `apply_to` overwrites
///   `Config.<field>` with `None`, mirroring upstream's
///   `hoistPattern != null` guard treating null as "feature disabled".
/// - `Some(Some(vec))` — explicit list. `apply_to` overwrites
///   `Config.<field>` with `Some(vec)`.
///
/// Stand-alone helper rather than reaching for `serde_with` (not in
/// the workspace deps) — the body is one line.
fn deserialize_double_option<'de, Value, De>(
    deserializer: De,
) -> Result<Option<Option<Value>>, De::Error>
where
    Value: Deserialize<'de>,
    De: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer).map(Some)
}

/// Settings readable from `pnpm-workspace.yaml`.
///
/// pnpm 10+ moved the bulk of its configuration (`storeDir`, `registry`,
/// `lockfile`, ...) out of `.npmrc` into `pnpm-workspace.yaml`, using
/// camelCase keys. Pacquet needs to honour these overrides so a real
/// pnpm-11-style project — where `.npmrc` may not even contain the
/// settings — works out of the box.
///
/// Every field is `Option` because the yaml is strictly additive on top of
/// [`Config`]: anything left unset falls through to whatever `.npmrc` provided
/// (or the hard-coded default).
///
/// See <https://pnpm.io/settings> for the canonical key list.
/// Non-config keys in a real pnpm-workspace.yaml (`packages`, `catalog`,
/// `catalogs`, `onlyBuiltDependencies`, `allowBuilds`, ...) are silently
/// ignored — serde drops them since the struct doesn't use
/// `deny_unknown_fields`.
///
/// pnpm v11 also reads `patchedDependencies` (and the other install
/// settings such as `allowBuilds`) from this file rather than from
/// `package.json`'s `pnpm` field — see upstream's
/// [`addSettingsFromWorkspaceManifestToConfig`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L803-L831),
/// which calls `getOptionsFromPnpmSettings` with the workspace
/// manifest. The misleadingly-named
/// [`getOptionsFromRootManifest.ts`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/getOptionsFromRootManifest.ts)
/// is wrapped at that call site, so its `manifestDir` parameter
/// actually carries the *workspace* dir.
#[derive(Debug, Default, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceSettings {
    pub hoist: Option<bool>,

    /// Tri-state `hoistPattern`. The deserializer wraps a plain
    /// `Option<Vec<String>>` in an extra `Some` so the three yaml
    /// states are distinguishable:
    ///
    /// - `None` — key absent in yaml → `apply_to` skips the field
    ///   (defaults stay).
    /// - `Some(None)` — explicit `hoistPattern: null` → `apply_to`
    ///   writes `Config.hoist_pattern = None`, disabling private
    ///   hoisting and contributing to the install-time
    ///   `is_some() || is_some()` short-circuit guard. Mirrors
    ///   upstream's `hoistPattern != null` semantics.
    /// - `Some(Some(vec))` — explicit list → `apply_to` writes
    ///   `Config.hoist_pattern = Some(vec)`.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub hoist_pattern: Option<Option<Vec<String>>>,

    /// Tri-state `publicHoistPattern`. Same semantics as
    /// [`Self::hoist_pattern`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub public_hoist_pattern: Option<Option<Vec<String>>>,
    pub shamefully_hoist: Option<bool>,
    pub store_dir: Option<String>,
    pub modules_dir: Option<String>,
    pub node_linker: Option<NodeLinker>,
    pub symlink: Option<bool>,
    pub virtual_store_dir: Option<String>,
    /// `enableGlobalVirtualStore` from `pnpm-workspace.yaml`. Default
    /// applied in [`Config`] is `false` — matches pnpm v11's
    /// effective default for non-`--global` installs (upstream's
    /// `true` assignment lives only inside the `pnpm install --global`
    /// branch, and pacquet has no `--global` flow). See
    /// [`Config::enable_global_virtual_store`].
    pub enable_global_virtual_store: Option<bool>,
    /// `globalVirtualStoreDir` from `pnpm-workspace.yaml`. Resolved
    /// against the workspace dir like the other path-valued fields.
    /// When set, overrides the derived `<store_dir>/links` path.
    pub global_virtual_store_dir: Option<String>,
    pub package_import_method: Option<PackageImportMethod>,
    pub modules_cache_max_age: Option<u64>,
    pub virtual_store_dir_max_length: Option<u64>,
    pub peers_suffix_max_length: Option<u64>,
    pub lockfile: Option<bool>,
    pub prefer_frozen_lockfile: Option<bool>,
    pub offline: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub lockfile_include_tarball_url: Option<bool>,
    pub registry: Option<String>,
    pub pnpr_server: Option<String>,

    /// User-defined named-registry aliases. Outer key is the alias
    /// name (`gh`, `work`, ...); inner string is the registry URL the
    /// alias resolves against. Merged on top of pnpm's built-in
    /// defaults at resolver construction. Mirrors upstream's
    /// [`namedRegistries`](https://github.com/pnpm/pnpm/blob/b61e268d57/config/reader/src/Config.ts#L227)
    /// setting.
    pub named_registries: Option<BTreeMap<String, String>>,

    pub auto_install_peers: Option<bool>,
    pub auto_install_peers_from_highest_match: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    /// `optimisticRepeatInstall` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. Defaults to `true` at the
    /// `Config` layer ([`Config::optimistic_repeat_install`]) to
    /// match pnpm.
    pub optimistic_repeat_install: Option<bool>,
    pub hoist_workspace_packages: Option<bool>,
    /// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Tri-state
    /// (`true | false | "deep"`) — see [`LinkWorkspacePackages`].
    pub link_workspace_packages: Option<LinkWorkspacePackages>,
    /// `injectWorkspacePackages` from `pnpm-workspace.yaml`. When
    /// `true`, every workspace-resolved dep is materialized as a
    /// `file:` (hard-linked copy) instead of a `link:` symlink. See
    /// [`Config::inject_workspace_packages`].
    pub inject_workspace_packages: Option<bool>,
    /// `hoistingLimits` from `pnpm-workspace.yaml`. One of `none`,
    /// `workspaces`, or `dependencies` — see
    /// [`crate::HoistingLimits`]. Missing → default
    /// [`crate::HoistingLimits::None`].
    pub hoisting_limits: Option<HoistingLimits>,
    /// `externalDependencies` from `pnpm-workspace.yaml`. Names
    /// whose top-level slot is reserved for an external linker
    /// and stripped from the hoist tree. Mirrors upstream's
    /// programmatic-only knob shape, exposed here as yaml for
    /// parity. Empty / missing → no externals.
    pub external_dependencies: Option<BTreeSet<String>>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub prefer_workspace_packages: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub strict_peer_dependencies: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub block_exotic_subdeps: Option<bool>,
    pub verify_store_integrity: Option<bool>,
    pub side_effects_cache: Option<bool>,
    pub side_effects_cache_readonly: Option<bool>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u64>,
    pub fetch_retry_maxtimeout: Option<u64>,
    pub network_concurrency: Option<usize>,
    pub fetch_timeout: Option<u64>,
    pub user_agent: Option<String>,
    /// `npmrcAuthFile` is read only from the global `config.yaml`
    /// (consumed by [`crate::Config::current`] to choose the user-level
    /// `.npmrc`); it is deliberately *not* in the `apply!` list, so a
    /// project `pnpm-workspace.yaml` declaring it is a no-op — matching
    /// pnpm, which sources the key from the global manifest only.
    pub npmrc_auth_file: Option<String>,

    /// Map of `name[@version]` → patch-file path (relative to the
    /// workspace dir or absolute). Read verbatim; relative-path
    /// resolution, file hashing, and grouping are deferred to
    /// [`pacquet_patching::resolve_and_group`] so the yaml layer
    /// stays pure data.
    ///
    /// [`IndexMap`] (not [`BTreeMap`]) — pnpm's JS-object iteration
    /// preserves the user's order, and that order leaks into
    /// `PATCH_KEY_CONFLICT` diagnostics that list matched ranges.
    /// Sorting the keys here would surface as a divergence in
    /// error messages.
    ///
    /// pnpm 10+ moved `patchedDependencies` out of
    /// `package.json#pnpm` into `pnpm-workspace.yaml`; pacquet
    /// matches that. The legacy `package.json#pnpm.patchedDependencies`
    /// shape is no longer consulted.
    ///
    /// [`BTreeMap`]: std::collections::BTreeMap
    pub patched_dependencies: Option<IndexMap<String, String>>,

    /// `configDependencies` from `pnpm-workspace.yaml`: package name →
    /// version-with-integrity spec. pnpm records this verbatim in the
    /// workspace-state file so that `checkDepsStatus` can detect when a
    /// config dependency changed and force a reinstall. Pacquet must
    /// write the same value back (see
    /// [`build_workspace_state`](../../package-manager/src/install.rs)),
    /// otherwise pnpm reads a missing `configDependencies` on the next
    /// `pnpm run` / `pnpm node`, compares it against the live config,
    /// and reinstalls on every invocation.
    pub config_dependencies: Option<BTreeMap<String, ConfigDependency>>,

    /// Map of `name[@version]` → `true` / `false`. Drives pnpm 11's
    /// default-deny build policy: a package's lifecycle scripts only
    /// run when an entry here resolves to `true`. Mirrors upstream's
    /// [`createAllowBuildFunction`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/src/index.ts).
    ///
    /// pnpm 10+ moved `allowBuilds` out of `package.json#pnpm` into
    /// `pnpm-workspace.yaml` alongside other install settings.
    pub allow_builds: Option<HashMap<String, bool>>,

    /// Bypass the [`allow_builds`] gate entirely — every package may
    /// run lifecycle scripts. Same `pnpm-workspace.yaml` migration
    /// as `allowBuilds`. Default `false`.
    ///
    /// [`allow_builds`]: Self::allow_builds
    pub dangerously_allow_all_builds: Option<bool>,

    /// `scriptsPrependNodePath` from `pnpm-workspace.yaml`. Tri-state
    /// — yaml accepts `true` / `false` / `"warn-only"`. Custom serde
    /// shape, see [`ScriptsPrependNodePath`]'s `Deserialize` impl.
    pub scripts_prepend_node_path: Option<ScriptsPrependNodePath>,

    /// `enablePrePostScripts` from `pnpm-workspace.yaml`. See
    /// [`Config::enable_pre_post_scripts`].
    pub enable_pre_post_scripts: Option<bool>,

    /// Tri-state `scriptShell` from `pnpm-workspace.yaml`. pnpm reads
    /// workspace settings into an object and assigns each present key
    /// onto the merged config (`addSettingsFromWorkspaceManifestToConfig`
    /// at <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L859-L885>),
    /// so an explicit `scriptShell: null` clears a value inherited from
    /// global `config.yaml`, while an absent key inherits. The extra
    /// `Option` layer preserves that distinction (same
    /// `deserialize_double_option` shape as `hoist_pattern`):
    ///
    /// - `None` — key absent → `apply_to` skips the field (inherit).
    /// - `Some(None)` — explicit `null` → `apply_to` writes
    ///   `Config.script_shell = None` (clear the inherited shell,
    ///   falling back to the platform default).
    /// - `Some(Some(s))` — explicit string → `apply_to` writes
    ///   `Config.script_shell = Some(s)`.
    ///
    /// See [`Config::script_shell`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub script_shell: Option<Option<String>>,

    /// Tri-state `nodeOptions` from `pnpm-workspace.yaml`. Same
    /// inherit / clear / set semantics as [`Self::script_shell`] — an
    /// explicit `nodeOptions: null` unsets an inherited `NODE_OPTIONS`.
    /// See [`Config::node_options`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub node_options: Option<Option<String>>,

    /// `unsafePerm` from `pnpm-workspace.yaml`. Forced to `true` on
    /// Windows in `apply_to` (matches upstream's
    /// `process.platform === 'win32'` override).
    pub unsafe_perm: Option<bool>,

    /// `childConcurrency` from `pnpm-workspace.yaml`. Resolved
    /// through [`crate::resolve_child_concurrency`] in `apply_to`.
    /// Signed `i32` here so negative values (interpreted as
    /// `parallelism - |value|`) round-trip cleanly.
    pub child_concurrency: Option<i32>,

    /// `workspaceConcurrency` from `pnpm-workspace.yaml` / global
    /// `config.yaml`. Resolved through
    /// [`crate::resolve_child_concurrency`] in `apply_to`, the same
    /// way `childConcurrency` is — both upstream settings run through
    /// [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34).
    /// Signed `i32` so negative values (interpreted as
    /// `parallelism - |value|`) round-trip cleanly. A genuine
    /// config-file key (so it is kept, not cleared, in
    /// [`Self::clear_workspace_only_fields`]).
    pub workspace_concurrency: Option<i32>,

    /// `gitShallowHosts` from `pnpm-workspace.yaml`. Overrides
    /// [`Config::git_shallow_hosts`] wholesale when set (mirrors
    /// pnpm's settings precedence, where `pnpm-workspace.yaml`
    /// replaces the built-in defaults rather than merging).
    pub git_shallow_hosts: Option<Vec<String>>,

    /// `supportedArchitectures` from `pnpm-workspace.yaml`. Drives the
    /// optional-dependency platform check at install time: a
    /// `name: ['darwin'], cpu: ['arm64']` setting tells pacquet to
    /// keep `darwin-arm64` variants of platform-tagged packages even
    /// on a non-matching host. Per-axis CLI flags (`--cpu`, `--libc`,
    /// `--os`) override individual axes — mirrors upstream's
    /// [`overrideSupportedArchitecturesWithCLI`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/overrideSupportedArchitecturesWithCLI.ts).
    /// Read from yaml verbatim (no `current` substitution here — that
    /// happens at the [`pacquet_package_is_installable::check_platform`]
    /// call site where the host triple is in scope).
    pub supported_architectures: Option<SupportedArchitectures>,

    /// `ignoredOptionalDependencies` from `pnpm-workspace.yaml`: a
    /// list of dep-name patterns whose matching entries get
    /// stripped from every manifest's `optionalDependencies` (and
    /// `dependencies`, when a package lists the same name in both)
    /// before any consumer sees them. Mirrors upstream's
    /// [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts)
    /// and the lockfile-side drift check at
    /// [`getOutdatedLockfileSetting.ts:58-60`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L58-L60).
    pub ignored_optional_dependencies: Option<Vec<String>>,

    /// `overrides` from `pnpm-workspace.yaml`: a `selector → spec`
    /// map that rewrites dependency specifiers everywhere they appear
    /// during install (both direct manifests and transitive
    /// packuments). Outer key encodes the override scope (bare name,
    /// `name@range`, or `parent>child` forms — see
    /// `pacquet_config_parse_overrides`); value is the replacement
    /// spec, or `-` to delete the dep entirely.
    ///
    /// Mirrors upstream's
    /// [`overrides`](https://github.com/pnpm/pnpm/blob/6d7903a8b7/config/reader/src/getOptionsFromRootManifest.ts#L18)
    /// shape — values are validated as strings at load time
    /// (`ERR_PNPM_INVALID_OVERRIDES`) and `$dep-name` self-references
    /// against the manifest's direct deps are resolved before
    /// downstream code sees them. Empty maps are normalized to
    /// `None` to match upstream's `delete settings.overrides`.
    ///
    /// pnpm 10+ moved `overrides` out of `package.json#pnpm` into
    /// `pnpm-workspace.yaml`. Pacquet matches that — the legacy
    /// `package.json#pnpm.overrides` shape is no longer consulted.
    ///
    /// Lockfile drift: the raw map is recorded in `pnpm-lock.yaml`'s
    /// `overrides:` field. On a subsequent install,
    /// `pacquet_lockfile::check_lockfile_settings` compares this
    /// against `lockfile.overrides` and raises `OverridesChanged`
    /// on mismatch. Mirrors upstream's
    /// [`getOutdatedLockfileSetting.ts:50-52`](https://github.com/pnpm/pnpm/blob/606f53e78f/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L50-L52).
    pub overrides: Option<IndexMap<String, String>>,

    /// `cacheDir` from `pnpm-workspace.yaml`. Resolved against the
    /// workspace dir like the other path-valued fields. Drives
    /// the lockfile-verified JSONL cache + packument mirror used
    /// by the verifier.
    pub cache_dir: Option<String>,

    /// `dlxCacheMaxAge` from `pnpm-workspace.yaml`. Minutes; see
    /// [`Config::dlx_cache_max_age`].
    pub dlx_cache_max_age: Option<u64>,

    /// `minimumReleaseAge` from `pnpm-workspace.yaml`. Milliseconds;
    /// see [`Config::minimum_release_age`].
    pub minimum_release_age: Option<u64>,

    /// `minimumReleaseAgeExclude` from `pnpm-workspace.yaml`.
    pub minimum_release_age_exclude: Option<Vec<String>>,

    /// `minimumReleaseAgeIgnoreMissingTime` from `pnpm-workspace.yaml`.
    pub minimum_release_age_ignore_missing_time: Option<bool>,

    /// `minimumReleaseAgeStrict` from `pnpm-workspace.yaml`.
    pub minimum_release_age_strict: Option<bool>,

    /// `trustLockfile` from `pnpm-workspace.yaml`. When `true`, the
    /// install skips the supply-chain verification pass entirely
    /// (see [`Config::trust_lockfile`]).
    ///
    /// [`Config::trust_lockfile`]: crate::Config::trust_lockfile
    pub trust_lockfile: Option<bool>,

    /// `trustPolicy` from `pnpm-workspace.yaml`. See [`TrustPolicy`].
    pub trust_policy: Option<TrustPolicy>,

    /// `trustPolicyExclude` from `pnpm-workspace.yaml`.
    pub trust_policy_exclude: Option<Vec<String>>,

    /// `trustPolicyIgnoreAfter` from `pnpm-workspace.yaml`. Minutes.
    pub trust_policy_ignore_after: Option<u64>,

    /// `packageExtensions` from `pnpm-workspace.yaml`: a
    /// `selector → extension` map that augments dependency manifests
    /// at install time. Outer key is a `name[@range]` selector; inner
    /// value lists the extra `dependencies`, `optionalDependencies`,
    /// `peerDependencies`, and `peerDependenciesMeta` entries to merge
    /// onto every matching manifest before the resolver walks it.
    ///
    /// Mirrors upstream's
    /// [`packageExtensions`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L145)
    /// shape and its consumer
    /// [`createPackageExtender`](https://github.com/pnpm/pnpm/blob/39101f5e37/hooks/read-package-hook/src/createPackageExtender.ts).
    /// `IndexMap` keeps insertion order so the hash-and-checksum side
    /// (a separate slice) can keep the same key ordering pnpm does.
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,

    /// `resolutionMode` from `pnpm-workspace.yaml`. See
    /// [`ResolutionMode`].
    pub resolution_mode: Option<ResolutionMode>,

    /// `catalogMode` from `pnpm-workspace.yaml`. See [`CatalogMode`].
    pub catalog_mode: Option<CatalogMode>,

    /// `registrySupportsTimeField` from `pnpm-workspace.yaml`. See
    /// [`Config::registry_supports_time_field`].
    ///
    /// [`Config::registry_supports_time_field`]: crate::Config::registry_supports_time_field
    pub registry_supports_time_field: Option<bool>,

    /// `allowedDeprecatedVersions` from `pnpm-workspace.yaml`. See
    /// [`Config::allowed_deprecated_versions`].
    ///
    /// [`Config::allowed_deprecated_versions`]: crate::Config::allowed_deprecated_versions
    pub allowed_deprecated_versions: Option<BTreeMap<String, String>>,

    /// `updateConfig` from `pnpm-workspace.yaml`. See [`UpdateConfig`].
    pub update_config: Option<UpdateConfig>,

    /// `peerDependencyRules` from `pnpm-workspace.yaml`. See
    /// [`PeerDependencyRules`].
    pub peer_dependency_rules: Option<PeerDependencyRules>,
}

/// `updateConfig` entry: settings that tune `pnpm update`. Today only
/// `ignoreDependencies` is modeled. Mirrors pnpm's
/// [`updateConfig`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L193-L195)
/// shape.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateConfig {
    /// Dependency-name patterns `pnpm update` skips. `createMatcher`
    /// glob/negation patterns upstream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_dependencies: Option<Vec<String>>,
}

/// `peerDependencyRules` entry: customizations applied when reporting
/// peer-dependency issues. Mirrors pnpm's
/// [`PeerDependencyRules`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L147-L151).
///
/// - `ignoreMissing` / `allowAny` are `createMatcher` glob/negation
///   pattern lists (matched against the peer package name).
/// - `allowedVersions` maps a peer selector (`name`, or the override
///   form `parent>name` / `parent@range>name`) to an extra semver range
///   that should be accepted.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PeerDependencyRules {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_missing: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_any: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_versions: Option<BTreeMap<String, String>>,
}

/// One `packageExtensions` entry: a subset of a manifest's dependency
/// groups, merged onto every matching manifest at install time. The
/// fields mirror pnpm's
/// [`PackageExtension = Pick<BaseManifest, 'dependencies' |
/// 'optionalDependencies' | 'peerDependencies' |
/// 'peerDependenciesMeta'>`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L145).
///
/// Read directly from yaml — no validation here beyond serde's shape
/// check. The hook
/// (`pacquet_package_manager::PackageExtender`) merges these onto
/// manifests, with the manifest's own fields taking precedence on
/// conflict so the extension never overwrites a value the package
/// already declared (mirrors upstream's `{ ...packageExtension[field],
/// ...manifest[field] }` spread order).
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

/// `peerDependenciesMeta` entry shape: a single `optional` flag today.
/// Mirrors upstream's
/// [`PeerDependencyMeta`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/misc.ts).
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PeerDependencyMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
}

/// Basename of the file pnpm reads; exported for test use.
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Basename of pnpm's global config file inside `<configDir>`. Matches
/// upstream's
/// [`GLOBAL_CONFIG_YAML_FILENAME`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/core/constants/src/index.ts#L12).
pub const GLOBAL_CONFIG_YAML_FILENAME: &str = "config.yaml";

/// Error when reading `pnpm-workspace.yaml`.
///
/// Pnpm's
/// [`workspace-manifest-reader`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/workspace-manifest-reader/src/index.ts)
/// treats `ENOENT` as "no manifest" and propagates every other failure.
/// Pacquet mirrors that split. `serde_saphyr::Error` is boxed so the
/// returned `Result` stays small.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadWorkspaceYamlError {
    #[display("Failed to read pnpm-workspace.yaml at {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to parse pnpm-workspace.yaml at {}: {source}", path.display())]
    ParseYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },
}

impl WorkspaceSettings {
    /// Read the global config.yaml at `<config_dir>/config.yaml`, if
    /// present.
    ///
    /// Pnpm v11's
    /// [`index.ts:228`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L228)
    /// reads this file with the same parser as `pnpm-workspace.yaml`,
    /// but applies it through a key-filter pass
    /// ([`isConfigFileKey`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/configFileKey.ts#L187))
    /// so workspace-only knobs (`nodeLinker`, `hoist`, `lockfile`, ...)
    /// cannot be set globally. Mirrors that filter via
    /// [`Self::clear_workspace_only_fields`].
    ///
    /// Returns `Ok(None)` when the file does not exist. Read or parse
    /// failures propagate.
    pub fn load_global(config_dir: &Path) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        let path = config_dir.join(GLOBAL_CONFIG_YAML_FILENAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LoadWorkspaceYamlError::ReadFile { path, source }),
        };
        let mut settings: WorkspaceSettings = serde_saphyr::from_str(&text)
            .map_err(Box::new)
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path, source })?;
        settings.clear_workspace_only_fields();
        Ok(Some(settings))
    }

    /// Zero out fields not permitted in the global `config.yaml`.
    ///
    /// Mirrors pnpm's
    /// [`isConfigFileKey`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/configFileKey.ts#L187)
    /// filter — every field listed here corresponds to a key in
    /// upstream's `excludedPnpmKeys`, plus the programmatic-only and
    /// workspace-only knobs (`patchedDependencies`, `allowBuilds`,
    /// `supportedArchitectures`, `ignoredOptionalDependencies`,
    /// `hoistingLimits`, `externalDependencies`) that pnpm only reads
    /// from `pnpm-workspace.yaml` or the legacy `package.json#pnpm`
    /// field. Without this filter a user could put `nodeLinker:
    /// hoisted` in `~/.config/pnpm/config.yaml` and pacquet would
    /// honor it while pnpm wouldn't — anti-parity.
    pub fn clear_workspace_only_fields(&mut self) {
        self.hoist = None;
        self.hoist_pattern = None;
        self.public_hoist_pattern = None;
        self.shamefully_hoist = None;
        self.modules_dir = None;
        self.node_linker = None;
        self.symlink = None;
        self.lockfile = None;
        self.offline = None;
        self.lockfile_include_tarball_url = None;
        self.auto_install_peers = None;
        self.auto_install_peers_from_highest_match = None;
        self.exclude_links_from_lockfile = None;
        self.hoist_workspace_packages = None;
        self.link_workspace_packages = None;
        self.inject_workspace_packages = None;
        self.dedupe_peer_dependents = None;
        self.dedupe_peers = None;
        self.dedupe_direct_deps = None;
        self.prefer_workspace_packages = None;
        self.dedupe_injected_deps = None;
        self.strict_peer_dependencies = None;
        self.resolve_peers_from_workspace_root = None;
        self.block_exotic_subdeps = None;
        self.hoisting_limits = None;
        self.external_dependencies = None;
        self.patched_dependencies = None;
        self.config_dependencies = None;
        self.allow_builds = None;
        self.supported_architectures = None;
        self.ignored_optional_dependencies = None;
        self.overrides = None;
        self.package_extensions = None;
    }

    /// Walk up from `start_dir` looking for a readable `pnpm-workspace.yaml`.
    /// Returns `Ok(None)` if no ancestor has one. Read or parse failures
    /// other than `ENOENT` propagate, matching pnpm's
    /// [`readManifestRaw`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/workspace-manifest-reader/src/index.ts).
    pub fn find_and_load(
        start_dir: &Path,
    ) -> Result<Option<(PathBuf, Self)>, LoadWorkspaceYamlError> {
        for dir in start_dir.ancestors() {
            let path = dir.join(WORKSPACE_MANIFEST_FILENAME);
            let read_result = fs::read_to_string(&path);

            // Walk up only when the read failed because nothing exists at
            // this level. Every other error (including `EISDIR` for a
            // directory named `pnpm-workspace.yaml`, or permission denied)
            // propagates, matching pnpm where `ENOENT` is the only silent
            // case.
            if let Err(error) = &read_result
                && error.kind() == ErrorKind::NotFound
            {
                continue;
            }

            let settings: WorkspaceSettings = read_result
                .map_err(|source| LoadWorkspaceYamlError::ReadFile { path: path.clone(), source })?
                .pipe_as_ref(serde_saphyr::from_str)
                .map_err(Box::new)
                .map_err(|source| LoadWorkspaceYamlError::ParseYaml {
                    path: path.clone(),
                    source,
                })?;

            return Ok(Some((path, settings)));
        }

        Ok(None)
    }

    /// Expand `${VAR}` in trusted user-controlled settings.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`].
    pub fn substitute_env_trusted<Sys: EnvVar>(&mut self) {
        self.substitute_env_scalars::<Sys>();
        substitute_optional_string::<Sys>(&mut self.pnpr_server);
        substitute_optional_string::<Sys>(&mut self.registry);
        substitute_optional_string_map::<Sys>(&mut self.named_registries);
    }

    /// Expand `${VAR}` in ordinary string settings, but drop
    /// placeholders inside workspace-controlled request-destination
    /// fields.
    /// The upstream
    /// [`replaceEnvInSettings`](https://github.com/pnpm/pnpm/blob/b61e268d57/config/reader/src/getOptionsFromRootManifest.ts#L66-L84)
    /// pass still runs `envReplace` on scalar strings while filtering
    /// `registry`, `registries`, `namedRegistries`, and `pnprServer`
    /// instead of expanding environment variables into request URLs.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`] and filtered values do not.
    pub fn substitute_env_untrusted<Sys: EnvVar>(&mut self) {
        self.substitute_env_scalars::<Sys>();

        if self.registry.as_deref().is_some_and(has_env_placeholder) {
            self.registry = None;
        }
        if let Some(named_registries) = self.named_registries.as_mut() {
            named_registries.retain(|_, value| !has_env_placeholder(value));
        }
        if self.pnpr_server.as_deref().is_some_and(has_env_placeholder) {
            self.pnpr_server = None;
        }
    }

    fn substitute_env_scalars<Sys: EnvVar>(&mut self) {
        substitute_optional_string::<Sys>(&mut self.store_dir);
        substitute_optional_string::<Sys>(&mut self.modules_dir);
        substitute_optional_string::<Sys>(&mut self.virtual_store_dir);
        substitute_optional_string::<Sys>(&mut self.global_virtual_store_dir);
        substitute_optional_string::<Sys>(&mut self.user_agent);
        substitute_optional_string::<Sys>(&mut self.npmrc_auth_file);
        substitute_optional_string::<Sys>(&mut self.cache_dir);
        substitute_optional_inner_string::<Sys>(&mut self.script_shell);
        substitute_optional_inner_string::<Sys>(&mut self.node_options);
    }

    /// Apply every set field onto `config`, leaving unset ones untouched.
    ///
    /// Path-valued fields (`store_dir`, `modules_dir`, `virtual_store_dir`)
    /// are resolved against `base_dir` if relative — anchored at the
    /// workspace root where the yaml was found, matching pnpm.
    pub fn apply_to(self, config: &mut Config, base_dir: &Path) {
        macro_rules! apply {
            ($($field:ident),* $(,)?) => {$(
                if let Some(v) = self.$field {
                    config.$field = v;
                }
            )*};
        }

        apply! {
            hoist, shamefully_hoist,
            node_linker, symlink, package_import_method, modules_cache_max_age,
            virtual_store_dir_max_length,
            peers_suffix_max_length,
            lockfile, prefer_frozen_lockfile, offline, prefer_offline,
            lockfile_include_tarball_url,
            auto_install_peers, auto_install_peers_from_highest_match,
            exclude_links_from_lockfile,
            optimistic_repeat_install,
            hoist_workspace_packages,
            hoisting_limits, external_dependencies,
            dedupe_peer_dependents, dedupe_peers, dedupe_direct_deps, dedupe_injected_deps,
            strict_peer_dependencies,
            resolve_peers_from_workspace_root, verify_store_integrity,
            block_exotic_subdeps,
            link_workspace_packages,
            inject_workspace_packages,
            prefer_workspace_packages,
            side_effects_cache, side_effects_cache_readonly,
            fetch_retries, fetch_retry_factor,
            fetch_retry_mintimeout, fetch_retry_maxtimeout,
            network_concurrency, fetch_timeout, user_agent,
            enable_global_virtual_store,
            git_shallow_hosts,
            resolution_mode, catalog_mode, registry_supports_time_field,
            allowed_deprecated_versions, update_config, peer_dependency_rules,
            enable_pre_post_scripts, dlx_cache_max_age,
        }

        // `hoist_pattern` and `public_hoist_pattern` carry the
        // tri-state described on [`deserialize_double_option`]:
        // outer `None` means "key missing — leave config defaults in
        // place"; outer `Some(inner)` means the user wrote something,
        // and `inner` is what they wrote (`None` for explicit null,
        // `Some(vec)` for a list). The inner value is assigned to
        // `Config.<field>` directly so an explicit `hoistPattern: null`
        // disables hoisting on that side via the install-time
        // `is_some() || is_some()` guard, matching upstream's
        // `!= null` semantics.
        if let Some(inner) = self.hoist_pattern {
            config.hoist_pattern = inner;
        }
        if let Some(inner) = self.public_hoist_pattern {
            config.public_hoist_pattern = inner;
        }

        // `hoist: false` nullifies `hoist_pattern` so the install-time
        // `is_some() || is_some()` guard short-circuits private hoisting
        // regardless of any explicit `hoist_pattern` the user (or
        // pacquet's defaults) supplied. Mirrors upstream's
        // [`projectConfig.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/projectConfig.ts#L72-L75)
        // — `result.hoist === false ⇒ hoistPattern: undefined`.
        // `publicHoistPattern` intentionally NOT nullified here:
        // upstream doesn't either; public hoisting is governed by
        // its own pattern + the legacy `shamefullyHoist` flag.
        // Applied AFTER `hoist_pattern` assignment so a yaml that sets
        // both `hoist: false` and `hoistPattern: ["..."]` still
        // disables — `hoist: false` wins, matching upstream.
        if !config.hoist {
            config.hoist_pattern = None;
        }

        if let Some(v) = self.modules_dir {
            config.modules_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.virtual_store_dir {
            config.virtual_store_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.global_virtual_store_dir {
            config.global_virtual_store_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.store_dir {
            config.store_dir = StoreDir::from(resolve(base_dir, &v));
        }
        if let Some(v) = self.registry {
            config.registry = if v.ends_with('/') { v } else { format!("{v}/") };
        }
        if let Some(v) = self.pnpr_server {
            config.pnpr_server = Some(v);
        }
        if let Some(v) = self.named_registries {
            config.named_registries = v;
        }

        // Anchor patch-file path resolution against the workspace dir
        // (the yaml's parent), matching upstream's
        // `getOptionsFromPnpmSettings(workspaceDir, ...)` at
        // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/getOptionsFromRootManifest.ts#L39-L46>.
        config.workspace_dir = Some(base_dir.to_path_buf());
        if let Some(v) = self.patched_dependencies {
            config.patched_dependencies = Some(v);
        }
        if let Some(v) = self.config_dependencies {
            config.config_dependencies = Some(v);
        }
        if let Some(v) = self.allow_builds {
            config.allow_builds = v;
        }
        if let Some(v) = self.dangerously_allow_all_builds {
            config.dangerously_allow_all_builds = v;
        }
        if let Some(v) = self.scripts_prepend_node_path {
            config.scripts_prepend_node_path = v;
        }
        // Tri-state: `Some(_)` (present in yaml) overwrites — including
        // `Some(None)` (explicit `null`), which clears the inherited
        // value. `None` (absent) leaves the inherited config untouched.
        if let Some(v) = self.script_shell {
            config.script_shell = v;
        }
        if let Some(v) = self.node_options {
            config.node_options = v;
        }
        if let Some(v) = self.unsafe_perm {
            config.unsafe_perm = v;
        }
        // Windows force-override (matches upstream's
        // [`process.platform === 'win32'`](https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L204-L220)
        // — running lifecycle scripts under a uid/gid drop is
        // POSIX-only).
        if cfg!(windows) {
            config.unsafe_perm = true;
        }
        // `childConcurrency: None` keeps the smart-default
        // `Config` constructor produced from
        // [`default_child_concurrency`]; `Some(n)` (including
        // negative) goes through the upstream resolver.
        if let Some(v) = self.child_concurrency {
            config.child_concurrency = resolve_child_concurrency(Some(v));
        }
        // `workspaceConcurrency` resolves through the same upstream
        // `getWorkspaceConcurrency` as `childConcurrency`; `None`
        // keeps the smart-default, `Some(n)` (including negative)
        // goes through the resolver.
        if let Some(v) = self.workspace_concurrency {
            config.workspace_concurrency = resolve_child_concurrency(Some(v));
        }
        if let Some(v) = self.supported_architectures {
            config.supported_architectures = Some(v);
        }
        if let Some(v) = self.ignored_optional_dependencies {
            config.ignored_optional_dependencies = Some(v);
        }
        // Empty overrides map collapses to `None` so the lockfile-side
        // drift check ignores it — mirrors upstream's
        // `delete settings.overrides` short-circuit in
        // [`getOptionsFromPnpmSettings`](https://github.com/pnpm/pnpm/blob/6d7903a8b7/config/reader/src/getOptionsFromRootManifest.ts#L32-L34).
        // The assignment runs whenever `self.overrides` is `Some(...)`
        // (even when empty) so an explicit `overrides: {}` at a later
        // layer (e.g. `PNPM_CONFIG_OVERRIDES={}` overlaid on top of a
        // non-empty workspace yaml) clears the inherited setting
        // instead of being a silent no-op. `$dep-name` self-reference
        // resolution happens elsewhere (the resolver chain), since it
        // needs the workspace's root manifest and that isn't in scope
        // here.
        if let Some(v) = self.overrides {
            config.overrides = (!v.is_empty()).then_some(v);
        }
        // Empty map collapses to `None` so the workspace-state drift
        // check ignores it, mirroring the same shape `overrides` uses.
        // An explicit later-layer `packageExtensions: {}` still clears
        // a prior non-empty value rather than no-oping.
        if let Some(v) = self.package_extensions {
            config.package_extensions = (!v.is_empty()).then_some(v);
        }
        if let Some(v) = self.cache_dir {
            config.cache_dir = resolve(base_dir, &v);
        }
        if let Some(v) = self.minimum_release_age {
            config.minimum_release_age = Some(v);
        }
        if let Some(v) = self.minimum_release_age_exclude {
            config.minimum_release_age_exclude = Some(v);
        }
        if let Some(v) = self.minimum_release_age_ignore_missing_time {
            config.minimum_release_age_ignore_missing_time = v;
        }
        if let Some(v) = self.minimum_release_age_strict {
            config.minimum_release_age_strict = Some(v);
        }
        if let Some(v) = self.trust_lockfile {
            config.trust_lockfile = v;
        }
        if let Some(v) = self.trust_policy {
            config.trust_policy = v;
        }
        if let Some(v) = self.trust_policy_exclude {
            config.trust_policy_exclude = Some(v);
        }
        if let Some(v) = self.trust_policy_ignore_after {
            config.trust_policy_ignore_after = Some(v);
        }
    }
}

fn has_env_placeholder(value: &str) -> bool {
    value
        .match_indices("${")
        .any(|(start, _)| value[start + 2..].find('}').is_some_and(|end| end > 0))
}

fn substitute_optional_string<Sys: EnvVar>(value: &mut Option<String>) {
    if let Some(value) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

fn substitute_optional_string_map<Sys: EnvVar>(value: &mut Option<BTreeMap<String, String>>) {
    if let Some(value) = value {
        for map_value in value.values_mut() {
            let (substituted, _) = env_replace_lossy::<Sys>(map_value);
            *map_value = substituted;
        }
    }
}

fn substitute_optional_inner_string<Sys: EnvVar>(value: &mut Option<Option<String>>) {
    if let Some(Some(value)) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

fn resolve(base: &Path, value: &str) -> PathBuf {
    let candidate = Path::new(value);
    if candidate.is_absolute() { candidate.to_path_buf() } else { base.join(candidate) }
}

fn find_workspace_manifest(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        let candidate = dir.join(WORKSPACE_MANIFEST_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

/// Resolve the workspace root for a given starting directory — i.e. the
/// directory containing the nearest ancestor `pnpm-workspace.yaml`.
/// Returns `start` itself if no manifest is found, so callers can always
/// use the result as a resolution base.
#[must_use]
pub fn workspace_root_or(start: &Path) -> PathBuf {
    find_workspace_manifest(start)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| start.to_path_buf())
}

#[cfg(test)]
mod tests;
