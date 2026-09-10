use super::{
    AllowBuild, AuditConfig, AuditLevel, AuditSettings, BTreeMap, BTreeSet, CargoSettings,
    CatalogMode, ConfigDependency, Deserialize, Deserializer, DroppedKeys, ErrorKind,
    GLOBAL_CONFIG_YAML_FILENAME, HashMap, HoistingLimits, IgnoredAny, IndexMap, InitType,
    LinkWorkspacePackages, LoadWorkspaceYamlError, NodeLinker, NodePackageMapType,
    PackageConfigsSetting, PackageExtension, PackageImportMethod, Path, PathBuf,
    PeerDependencyRules, Pipe, PmOnFail, PnpmfileSetting, PythonSettings, RegistryEntry,
    RemoteSideEffectsCacheSettings, ResolutionMode, RuntimeOnFail, SCHEMA_DIRECTIVE_KEY,
    SaveWorkspaceProtocol, ScriptsPrependNodePath, SideEffectsCacheSetting, SupportedArchitectures,
    TaskSettings, TrustPolicy, UpdateConfig, UpdateSettings, VerifyDepsBeforeRun, VirtualStoreType,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceKeyIssues, fs, redact_and_sanitize,
};

/// `serde` helper for fields that need to distinguish "missing key"
/// from "explicit null" in YAML / JSON.
///
/// Stand-alone helper rather than reaching for `serde_with` (not in
/// the workspace deps) — the body is one line.
pub(super) fn deserialize_double_option<'de, Value, De>(
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
/// [`Config`](crate::settings::Config): anything left unset falls through to whatever `.npmrc` provided
/// (or the hard-coded default).
///
/// See <https://pnpm.io/settings> for the canonical key list.
/// Workspace-structural keys (`packages`, `catalog`, `catalogs`, the build
/// allowlists) are carried only for `pnpm config get` / `list` — see
/// [`Self::packages`]. Anything else that is not a field is silently
/// ignored — serde drops it since the struct doesn't use
/// `deny_unknown_fields`.
///
/// pnpm v11 also reads `patchedDependencies` (and the other install
/// settings such as `allowBuilds`) from this file rather than from
/// `package.json`'s `pnpm` field, resolving those settings against the
/// workspace dir.
#[derive(Debug, Default, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceSettings {
    pub bail: Option<bool>,
    pub ci: Option<bool>,
    pub update_notifier: Option<bool>,
    pub color: Option<crate::ColorMode>,
    pub embed_readme: Option<bool>,
    pub ignore_workspace_root_check: Option<bool>,
    pub optional: Option<bool>,
    pub package_lock: Option<bool>,
    pub pending: Option<bool>,
    pub recursive_install: Option<bool>,
    pub reverse: Option<bool>,
    pub stream: Option<bool>,
    pub aggregate_output: Option<bool>,
    pub reporter_hide_prefix: Option<bool>,
    pub use_stderr: Option<bool>,
    pub ignore_workspace: Option<bool>,
    pub shell_emulator: Option<bool>,
    pub skip_manifest_obfuscation: Option<bool>,
    pub sort: Option<bool>,
    pub use_beta_cli: Option<bool>,
    pub hoist: Option<bool>,

    /// Tri-state `hoistPattern` — see `deserialize_double_option`.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub hoist_pattern: Option<Option<Vec<String>>>,

    /// Tri-state `publicHoistPattern`. Same semantics as
    /// [`Self::hoist_pattern`].
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub public_hoist_pattern: Option<Option<Vec<String>>>,
    pub shamefully_hoist: Option<bool>,
    pub store_dir: Option<String>,
    pub state_dir: Option<String>,
    pub modules_dir: Option<String>,
    pub node_linker: Option<NodeLinker>,
    pub node_experimental_package_map: Option<bool>,
    pub node_package_map_type: Option<NodePackageMapType>,
    pub symlink: Option<bool>,
    pub virtual_store_dir: Option<String>,
    /// `virtualStoreType` from `pnpm-workspace.yaml`. See
    /// [`crate::VirtualStoreType`], and
    /// [`Config::enable_global_virtual_store`](crate::settings::Config::enable_global_virtual_store) for the default.
    pub virtual_store_type: Option<VirtualStoreType>,
    /// `enableGlobalVirtualStore`, the boolean spelling of
    /// [`Self::virtual_store_type`]. A file may carry either or both; the
    /// canonical key wins.
    pub enable_global_virtual_store: Option<bool>,
    /// `virtualStoreOnly` from `pnpm-workspace.yaml`. See
    /// [`Config::virtual_store_only`](crate::settings::Config::virtual_store_only).
    pub virtual_store_only: Option<bool>,
    /// `globalShims` from `pnpm-workspace.yaml` or the global
    /// `config.yaml`. One layer of the record; merged key-wise into
    /// [`Config::global_shims`](crate::settings::Config::global_shims) rather than assigned
    /// wholesale. See [`crate::GlobalShims`].
    pub global_shims: Option<crate::GlobalShimsSetting>,
    /// `enableModulesDir` from `pnpm-workspace.yaml`. See
    /// [`Config::enable_modules_dir`](crate::settings::Config::enable_modules_dir).
    pub enable_modules_dir: Option<bool>,
    /// `globalVirtualStoreDir` from `pnpm-workspace.yaml`. Resolved
    /// against the workspace dir like the other path-valued fields.
    /// When set, overrides the derived `<store_dir>/links` path.
    pub global_virtual_store_dir: Option<String>,
    /// `globalDir` from the global `config.yaml` or the environment. A
    /// relative value resolves against the directory pnpm runs in, which
    /// is where pnpm itself resolves it. See [`Config::global_dir`](crate::settings::Config::global_dir).
    ///
    /// No repo-committed file may set it — see [`crate::refused_keys`].
    pub global_dir: Option<String>,
    /// `globalBinDir` from the global `config.yaml` or the environment. A
    /// relative value resolves against the directory pnpm runs in, which
    /// is where pnpm itself resolves it. See [`Config::global_bin_dir`](crate::settings::Config::global_bin_dir).
    ///
    /// No repo-committed file may set it — see [`crate::refused_keys`].
    pub global_bin_dir: Option<String>,
    pub package_import_method: Option<PackageImportMethod>,
    pub modules_cache_max_age: Option<u64>,
    pub virtual_store_dir_max_length: Option<u64>,
    pub peers_suffix_max_length: Option<u64>,
    pub lockfile: Option<bool>,
    /// `lockfileDir` from `pnpm-workspace.yaml` or the global
    /// `config.yaml`. Resolved against the workspace dir like the other
    /// path-valued fields. See [`Config::lockfile_dir`](crate::settings::Config::lockfile_dir).
    pub lockfile_dir: Option<String>,
    pub prefer_frozen_lockfile: Option<bool>,

    /// `frozenLockfile` from `pnpm-workspace.yaml`. Unset by default:
    /// see [`Config::frozen_lockfile`](crate::settings::Config::frozen_lockfile).
    pub frozen_lockfile: Option<bool>,
    pub deploy_all_files: Option<bool>,
    pub force_legacy_deploy: Option<bool>,
    pub shared_workspace_lockfile: Option<bool>,
    pub git_branch_lockfile: Option<bool>,
    pub merge_git_branch_lockfiles: Option<bool>,
    pub merge_git_branch_lockfiles_branch_pattern: Option<Vec<String>>,
    pub offline: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub lockfile_include_tarball_url: Option<bool>,
    pub registry: Option<String>,
    pub scope: Option<String>,
    /// The registries the project declares. Keyed by registry URL, with the
    /// routes to each registry inside its entry; a map of plain strings is the
    /// older `<scope>: <url>` shape and is read as one.
    pub registries: Option<BTreeMap<String, RegistryEntry>>,
    pub pnpr_server: Option<String>,
    pub cargo: Option<CargoSettings>,
    pub python: Option<PythonSettings>,
    pub remote_side_effects_cache: Option<RemoteSideEffectsCacheSettings>,
    pub https_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub no_proxy: Option<serde_json::Value>,
    pub proxy: Option<String>,
    pub noproxy: Option<serde_json::Value>,

    /// User-defined named-registry aliases. Outer key is the alias
    /// name (`gh`, `work`, ...); inner string is the registry URL the
    /// alias resolves against. Merged on top of pnpm's built-in
    /// defaults at resolver construction.
    ///
    /// Deprecated in favor of the `prefix` field of a
    /// [`crate::RegistryDeclaration`],
    /// and only read for the prefixes `registries` does not declare.
    pub named_registries: Option<BTreeMap<String, String>>,

    /// Structured registry auth (`_auth`). Honored **only** from the global
    /// pnpm `config.yaml` (read via `NpmrcAuth::from_json_sources`, not
    /// applied in [`Self::apply_to`]) — never a project file, so repo config
    /// can't supply credentials. A raw [`serde_json::Value`] so the auth
    /// parser is the single validator of its shape.
    #[serde(rename = "_auth")]
    pub auth: Option<serde_json::Value>,

    pub auto_install_peers: Option<bool>,
    pub auto_install_peers_from_highest_match: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    /// `optimisticRepeatInstall` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. Defaults to `true` at the
    /// `Config` layer ([`Config::optimistic_repeat_install`](crate::settings::Config::optimistic_repeat_install)) to
    /// match pnpm.
    pub optimistic_repeat_install: Option<bool>,
    pub hoist_workspace_packages: Option<bool>,
    /// `extendNodePath` from `pnpm-workspace.yaml`. See
    /// [`Config::extend_node_path`](crate::settings::Config::extend_node_path).
    pub extend_node_path: Option<bool>,
    /// `preferSymlinkedExecutables` from `pnpm-workspace.yaml`. Unset by
    /// default: see [`Config::prefer_symlinked_executables`](crate::settings::Config::prefer_symlinked_executables).
    pub prefer_symlinked_executables: Option<bool>,
    /// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Tri-state
    /// (`true | false | "deep"`) — see [`LinkWorkspacePackages`].
    pub link_workspace_packages: Option<LinkWorkspacePackages>,
    /// `saveWorkspaceProtocol` from `pnpm-workspace.yaml`. Tri-state
    /// (`true | false | "rolling"`) — see [`SaveWorkspaceProtocol`].
    pub save_workspace_protocol: Option<SaveWorkspaceProtocol>,
    /// `injectWorkspacePackages` from `pnpm-workspace.yaml`. When
    /// `true`, every workspace-resolved dep is materialized as a
    /// `file:` (hard-linked copy) instead of a `link:` symlink. See
    /// [`Config::inject_workspace_packages`](crate::settings::Config::inject_workspace_packages).
    pub inject_workspace_packages: Option<bool>,
    /// `hoistingLimits` from `pnpm-workspace.yaml`. One of `none`,
    /// `workspaces`, or `dependencies` — see
    /// [`crate::HoistingLimits`]. Missing → default
    /// [`crate::HoistingLimits::None`].
    pub hoisting_limits: Option<HoistingLimits>,
    /// `externalDependencies` from `pnpm-workspace.yaml`. Names
    /// whose top-level slot is reserved for an external linker
    /// and stripped from the hoist tree. Empty / missing → no
    /// externals.
    pub external_dependencies: Option<BTreeSet<String>>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub prefer_workspace_packages: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub strict_peer_dependencies: Option<bool>,
    pub ignore_compatibility_db: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub block_exotic_subdeps: Option<bool>,
    pub verify_store_integrity: Option<bool>,
    pub strict_store_pkg_content_check: Option<bool>,
    pub include_workspace_root: Option<bool>,
    pub ignore_workspace_cycles: Option<bool>,
    pub disallow_workspace_cycles: Option<bool>,
    /// `frozenStore` from `pnpm-workspace.yaml`. Opens the store
    /// read-only and suppresses every store write — see
    /// [`Config::frozen_store`]. Default `false`.
    ///
    /// [`Config::frozen_store`]: crate::Config::frozen_store
    pub frozen_store: Option<bool>,
    /// `sideEffectsCache`: whether a build is restored, whether one is saved,
    /// and where from. A bare boolean sets reading and writing together.
    pub side_effects_cache: Option<SideEffectsCacheSetting>,
    /// The boolean spelling of `sideEffectsCache: { read: true, write: false }`.
    pub side_effects_cache_readonly: Option<bool>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u64>,
    pub fetch_retry_maxtimeout: Option<u64>,
    pub network_concurrency: Option<usize>,
    /// `maxSockets` — per-origin concurrent-connection cap. See
    /// [`Config::max_sockets`](crate::settings::Config::max_sockets). Default unset (no per-origin cap).
    pub max_sockets: Option<usize>,
    /// `maxsockets` — npm's spelling of [`Self::max_sockets`], which pnpm
    /// reads too. A field of its own rather than a serde alias, because a
    /// file carrying both spellings is a duplicate field to serde and
    /// would fail the whole parse; pnpm takes it and lets the canonical
    /// spelling win.
    pub maxsockets: Option<usize>,
    pub fetch_timeout: Option<u64>,
    /// The `fetchWarnTimeoutMs` YAML value in milliseconds. [`None`] leaves
    /// [`Config::fetch_warn_timeout_ms`](crate::settings::Config::fetch_warn_timeout_ms) unchanged.
    pub fetch_warn_timeout_ms: Option<u64>,
    /// The `fetchMinSpeedKiBps` YAML value in KiB/s. [`None`] leaves
    /// [`Config::fetch_min_speed_ki_bps`](crate::settings::Config::fetch_min_speed_ki_bps) unchanged.
    pub fetch_min_speed_ki_bps: Option<u64>,
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
    /// [`pnpm_patching::resolve_and_group`] so the yaml layer
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

    pub patches_dir: Option<String>,

    pub pnpmfile: Option<PnpmfileSetting>,

    /// `globalPnpmfile`. Unlike [`Self::pnpmfile`] this survives
    /// [`Self::clear_workspace_only_fields`]: pnpm lists `global-pnpmfile`
    /// among the keys its global `config.yaml` accepts.
    pub global_pnpmfile: Option<String>,

    /// `allowUnusedPatches` from `pnpm-workspace.yaml`. Default `false`.
    pub allow_unused_patches: Option<bool>,

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

    /// Map of `name[@version]` → [`AllowBuild`]. Drives pnpm 11's
    /// default-deny build policy: a package's lifecycle scripts only
    /// run when an entry here resolves to `true`.
    ///
    /// pnpm 10+ moved `allowBuilds` out of `package.json#pnpm` into
    /// `pnpm-workspace.yaml` alongside other install settings.
    pub allow_builds: Option<HashMap<String, AllowBuild>>,

    /// The workspace-structural keys of `pnpm-workspace.yaml`, carried so
    /// `pnpm config get` / `pnpm config list` can show them. Installs read
    /// them from the workspace-manifest layer, not from [`Config`](crate::settings::Config), so
    /// [`Self::apply_to`] leaves them alone and the global `config.yaml`
    /// refuses them.
    pub packages: Option<Vec<String>>,
    /// See [`Self::packages`].
    pub catalog: Option<IndexMap<String, String>>,
    /// See [`Self::packages`].
    pub catalogs: Option<IndexMap<String, IndexMap<String, String>>>,
    /// See [`Self::packages`].
    pub only_built_dependencies: Option<Vec<String>>,
    /// See [`Self::packages`].
    pub never_built_dependencies: Option<Vec<String>>,
    /// See [`Self::packages`].
    pub ignored_built_dependencies: Option<Vec<String>>,

    /// Bypass the [`allow_builds`] gate entirely — every package may
    /// run lifecycle scripts. Same `pnpm-workspace.yaml` migration
    /// as `allowBuilds`. Default `false`.
    ///
    /// [`allow_builds`]: Self::allow_builds
    pub dangerously_allow_all_builds: Option<bool>,

    /// `strictDepBuilds` from `pnpm-workspace.yaml`. When `true` (the
    /// default), an install that ignored any dependency build script
    /// fails instead of only warning. Default `true`.
    pub strict_dep_builds: Option<bool>,

    /// `ignoreScripts` from `pnpm-workspace.yaml`. When `true`, no
    /// lifecycle scripts run and ignored dependency builds aren't
    /// collected. See [`Config::ignore_scripts`](crate::settings::Config::ignore_scripts). The `--ignore-scripts`
    /// CLI flag ORs on top of this. Default `false`.
    pub ignore_scripts: Option<bool>,

    /// `ignorePnpmfile` from `pnpm-workspace.yaml`. When `true`, no pnpmfile
    /// hooks run. See [`Config::ignore_pnpmfile`](crate::settings::Config::ignore_pnpmfile). The `--ignore-pnpmfile` CLI
    /// flag ORs on top of this. Cleared by
    /// [`Self::clear_workspace_only_fields`], so the global `config.yaml`
    /// cannot set it. Default `false`.
    pub ignore_pnpmfile: Option<bool>,

    /// `gitChecks` from `pnpm-workspace.yaml`. When `false`, `pnpm publish`
    /// skips its git working-tree checks. See [`Config::git_checks`](crate::settings::Config::git_checks). The
    /// `--no-git-checks` CLI flag forces it off on top of this. Default
    /// `true`.
    pub git_checks: Option<bool>,

    /// `engineStrict` from `pnpm-workspace.yaml` / global `config.yaml`.
    /// See [`Config::engine_strict`](crate::settings::Config::engine_strict). Default `false`.
    pub engine_strict: Option<bool>,

    /// `nodeVersion` from `pnpm-workspace.yaml` / global `config.yaml`.
    /// See [`Config::node_version`](crate::settings::Config::node_version). Default unset (auto-detect).
    pub node_version: Option<String>,

    /// `runtimeOnFail` from `pnpm-workspace.yaml` / global `config.yaml`.
    pub runtime_on_fail: Option<RuntimeOnFail>,

    /// Per-release-channel Node.js download mirrors.
    pub node_download_mirrors: Option<HashMap<String, String>>,

    /// `scriptsPrependNodePath` from `pnpm-workspace.yaml`. Tri-state
    /// — yaml accepts `true` / `false` / `"warn-only"`. Custom serde
    /// shape, see [`ScriptsPrependNodePath`]'s `Deserialize` impl.
    pub scripts_prepend_node_path: Option<ScriptsPrependNodePath>,

    /// `enablePrePostScripts` from `pnpm-workspace.yaml`. See
    /// [`Config::enable_pre_post_scripts`](crate::settings::Config::enable_pre_post_scripts).
    pub enable_pre_post_scripts: Option<bool>,

    /// Tri-state `scriptShell` from `pnpm-workspace.yaml`. pnpm reads
    /// workspace settings into an object and assigns each present key
    /// onto the merged config, so an explicit `scriptShell: null`
    /// clears a value inherited from global `config.yaml`, while an
    /// absent key inherits. The extra `Option` layer preserves that
    /// distinction (same `deserialize_double_option` shape as
    /// `hoist_pattern`).
    ///
    /// See [`Config::script_shell`](crate::settings::Config::script_shell).
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub script_shell: Option<Option<String>>,

    /// Tri-state `nodeOptions` from `pnpm-workspace.yaml`. Same
    /// inherit / clear / set semantics as [`Self::script_shell`] — an
    /// explicit `nodeOptions: null` unsets an inherited `NODE_OPTIONS`.
    /// See [`Config::node_options`](crate::settings::Config::node_options).
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub node_options: Option<Option<String>>,

    /// `unsafePerm` from `pnpm-workspace.yaml`. Forced to `true` on
    /// Windows in `apply_to`, matching pnpm.
    pub unsafe_perm: Option<bool>,

    /// `childConcurrency` from `pnpm-workspace.yaml`. Resolved
    /// through [`crate::resolve_child_concurrency`] in `apply_to`.
    /// Signed `i32` here so negative values (interpreted as
    /// `parallelism - |value|`) round-trip cleanly.
    pub child_concurrency: Option<i32>,

    /// `workspaceConcurrency` from `pnpm-workspace.yaml` / global
    /// `config.yaml`. Resolved through
    /// [`crate::resolve_child_concurrency`] in `apply_to`, the same
    /// way `childConcurrency` is. Signed `i32` so negative values
    /// (interpreted as `parallelism - |value|`) round-trip cleanly.
    /// A genuine config-file key (so it is kept, not cleared, in
    /// [`Self::clear_workspace_only_fields`]).
    pub workspace_concurrency: Option<i32>,

    /// `gitShallowHosts` from `pnpm-workspace.yaml`. Overrides
    /// [`Config::git_shallow_hosts`](crate::settings::Config::git_shallow_hosts) wholesale when set —
    /// `pnpm-workspace.yaml` replaces the built-in defaults rather
    /// than merging.
    pub git_shallow_hosts: Option<Vec<String>>,

    /// `testPattern` from `pnpm-workspace.yaml` — see
    /// [`Config::test_pattern`](crate::settings::Config::test_pattern).
    pub test_pattern: Option<Vec<String>>,

    /// `changedFilesIgnorePattern` from `pnpm-workspace.yaml` — see
    /// [`Config::changed_files_ignore_pattern`](crate::settings::Config::changed_files_ignore_pattern).
    pub changed_files_ignore_pattern: Option<Vec<String>>,

    /// `legacyDirFiltering` from `pnpm-workspace.yaml` — see
    /// [`Config::legacy_dir_filtering`].
    ///
    /// [`Config::legacy_dir_filtering`]: crate::Config::legacy_dir_filtering
    pub legacy_dir_filtering: Option<bool>,

    /// `syncInjectedDepsAfterScripts` from `pnpm-workspace.yaml` — see
    /// [`Config::sync_injected_deps_after_scripts`](crate::settings::Config::sync_injected_deps_after_scripts).
    pub sync_injected_deps_after_scripts: Option<Vec<String>>,

    /// `supportedArchitectures` from `pnpm-workspace.yaml`. Drives the
    /// optional-dependency platform check at install time: a
    /// `name: ['darwin'], cpu: ['arm64']` setting tells pacquet to
    /// keep `darwin-arm64` variants of platform-tagged packages even
    /// on a non-matching host. Per-axis CLI flags (`--cpu`, `--libc`,
    /// `--os`) override individual axes.
    /// Read from yaml verbatim (no `current` substitution here — that
    /// happens at the [`pnpm_package_is_installable::check_platform`]
    /// call site where the host triple is in scope).
    pub supported_architectures: Option<SupportedArchitectures>,

    /// `ignoredOptionalDependencies` from `pnpm-workspace.yaml`: a
    /// list of dep-name patterns whose matching entries get
    /// stripped from every manifest's `optionalDependencies` (and
    /// `dependencies`, when a package lists the same name in both)
    /// before any consumer sees them. The setting also participates
    /// in the lockfile-side drift check.
    pub ignored_optional_dependencies: Option<Vec<String>>,

    /// `overrides` from `pnpm-workspace.yaml`: a `selector → spec`
    /// map that rewrites dependency specifiers everywhere they appear
    /// during install (both direct manifests and transitive
    /// packuments). Outer key encodes the override scope (bare name,
    /// `name@range`, or `parent>child` forms — see
    /// `pnpm_config_parse_overrides`); value is the replacement
    /// spec, or `-` to delete the dep entirely.
    ///
    /// Values are validated as strings at load time
    /// (`ERR_PNPM_INVALID_OVERRIDES`) and `$dep-name` self-references
    /// against the manifest's direct deps are resolved before
    /// downstream code sees them. Empty maps are normalized to
    /// `None` so the overrides key is dropped entirely.
    ///
    /// pnpm 10+ moved `overrides` out of `package.json#pnpm` into
    /// `pnpm-workspace.yaml`. Pacquet matches that — the legacy
    /// `package.json#pnpm.overrides` shape is no longer consulted.
    ///
    /// Lockfile drift: the raw map is recorded in `pnpm-lock.yaml`'s
    /// `overrides:` field. On a subsequent install,
    /// `pnpm_lockfile::check_lockfile_settings` compares this
    /// against `lockfile.overrides` and raises `OverridesChanged`
    /// on mismatch.
    pub overrides: Option<IndexMap<String, String>>,

    /// `cacheDir` from `pnpm-workspace.yaml`. Resolved against the
    /// workspace dir like the other path-valued fields. Drives
    /// the lockfile-verified JSONL cache + packument mirror used
    /// by the verifier.
    pub cache_dir: Option<String>,

    /// `dlxCacheMaxAge` from `pnpm-workspace.yaml`. Minutes; see
    /// [`Config::dlx_cache_max_age`](crate::settings::Config::dlx_cache_max_age).
    pub dlx_cache_max_age: Option<u64>,

    /// `minimumReleaseAge` from `pnpm-workspace.yaml`. Milliseconds;
    /// see [`Config::minimum_release_age`](crate::settings::Config::minimum_release_age).
    pub minimum_release_age: Option<u64>,

    /// `minimumReleaseAgeExclude` from `pnpm-workspace.yaml`.
    pub minimum_release_age_exclude: Option<Vec<String>>,

    /// `minimumReleaseAgeExcludePrune` from `pnpm-workspace.yaml`.
    /// See [`Config::minimum_release_age_exclude_prune`](crate::settings::Config::minimum_release_age_exclude_prune). Default
    /// `false`.
    pub minimum_release_age_exclude_prune: Option<bool>,

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

    /// `initPackageManager` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See
    /// [`Config::init_package_manager`].
    ///
    /// [`Config::init_package_manager`]: crate::Config::init_package_manager
    pub init_package_manager: Option<bool>,

    /// `initType` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`InitType`].
    pub init_type: Option<InitType>,

    /// `initAuthorName` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_author_name`].
    ///
    /// [`Config::init_author_name`]: crate::Config::init_author_name
    pub init_author_name: Option<String>,

    /// `initAuthorEmail` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_author_email`].
    ///
    /// [`Config::init_author_email`]: crate::Config::init_author_email
    pub init_author_email: Option<String>,

    /// `initAuthorUrl` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_author_url`].
    ///
    /// [`Config::init_author_url`]: crate::Config::init_author_url
    pub init_author_url: Option<String>,

    /// `initLicense` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_license`].
    ///
    /// [`Config::init_license`]: crate::Config::init_license
    pub init_license: Option<String>,

    /// `initVersion` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`Config::init_version`].
    ///
    /// [`Config::init_version`]: crate::Config::init_version
    pub init_version: Option<String>,

    /// `pmOnFail` from `pnpm-workspace.yaml`. See [`PmOnFail`].
    pub pm_on_fail: Option<PmOnFail>,

    /// `verifyDepsBeforeRun` from `pnpm-workspace.yaml` /
    /// `~/.config/pnpm/config.yaml`. See [`VerifyDepsBeforeRun`].
    pub verify_deps_before_run: Option<VerifyDepsBeforeRun>,

    /// `audit` from `pnpm-workspace.yaml`. Supersedes `auditLevel` and
    /// `auditConfig`; see [`AuditSettings`]. When both a value and its
    /// deprecated counterpart are set, `audit` wins (with a warning) —
    /// the mapping onto [`Config::audit_level`](crate::settings::Config::audit_level) / [`Config::audit_config`](crate::settings::Config::audit_config)
    /// happens in [`Self::apply_to`].
    pub audit: Option<AuditSettings>,

    /// `auditLevel` from `pnpm-workspace.yaml`.
    ///
    /// Deprecated in favor of [`AuditSettings::level`], kept for backward
    /// compatibility until the next major version.
    pub audit_level: Option<AuditLevel>,

    /// `auditConfig` from `pnpm-workspace.yaml`.
    ///
    /// Deprecated in favor of [`AuditSettings::ignore`], kept for backward
    /// compatibility until the next major version.
    pub audit_config: Option<AuditConfig>,

    /// `versioning` from `pnpm-workspace.yaml`: native workspace release
    /// management (fixed groups, ignore list, maxBump cap, per-package
    /// prerelease lines, changelog settings).
    pub versioning: Option<pnpm_versioning::VersioningSettings>,

    /// `trustPolicyExclude` from `pnpm-workspace.yaml`.
    pub trust_policy_exclude: Option<Vec<String>>,

    /// `trustPolicyExcludePrune` from `pnpm-workspace.yaml`.
    /// See [`Config::trust_policy_exclude_prune`](crate::settings::Config::trust_policy_exclude_prune). Default `false`.
    pub trust_policy_exclude_prune: Option<bool>,

    /// `trustPolicyIgnoreAfter` from `pnpm-workspace.yaml`. Minutes.
    pub trust_policy_ignore_after: Option<u64>,

    /// `packageExtensions` from `pnpm-workspace.yaml`: a
    /// `selector → extension` map that augments dependency manifests
    /// at install time. Outer key is a `name[@range]` selector; inner
    /// value lists the extra `dependencies`, `optionalDependencies`,
    /// `peerDependencies`, and `peerDependenciesMeta` entries to merge
    /// onto every matching manifest before the resolver walks it.
    ///
    /// `IndexMap` keeps insertion order so the hash-and-checksum side
    /// (a separate slice) can keep the same key ordering pnpm does.
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,

    /// `packageConfigs` from `pnpm-workspace.yaml`: settings that
    /// apply to one project of the workspace instead of all of them.
    /// See [`PackageConfigsSetting`] for the two spellings and
    /// [`Config::anchor_dedicated_project`] for where the settings
    /// are applied.
    ///
    /// [`Config::anchor_dedicated_project`]: crate::Config::anchor_dedicated_project
    pub package_configs: Option<PackageConfigsSetting>,

    /// `resolutionMode` from `pnpm-workspace.yaml`. See
    /// [`ResolutionMode`].
    pub resolution_mode: Option<ResolutionMode>,

    /// `catalogMode` from `pnpm-workspace.yaml`. See [`CatalogMode`].
    pub catalog_mode: Option<CatalogMode>,

    /// `catalogPrune` from `pnpm-workspace.yaml`. See
    /// [`Config::catalog_prune`](crate::settings::Config::catalog_prune). Default `false`.
    pub catalog_prune: Option<bool>,

    /// `catalogPrune`'s former name, still accepted. [`Self::catalog_prune`]
    /// wins when a file carries both.
    pub cleanup_unused_catalogs: Option<bool>,

    /// `saveCatalogName` from `pnpm-workspace.yaml`. See
    /// [`Config::save_catalog_name`].
    ///
    /// [`Config::save_catalog_name`]: crate::Config::save_catalog_name
    pub save_catalog_name: Option<String>,

    /// `savePrefix` from `pnpm-workspace.yaml`. See
    /// [`Config::save_prefix`].
    ///
    /// [`Config::save_prefix`]: crate::Config::save_prefix
    pub save_prefix: Option<String>,

    /// `saveExact` from `pnpm-workspace.yaml`. See
    /// [`Config::save_exact`]. Default `false`.
    ///
    /// [`Config::save_exact`]: crate::Config::save_exact
    pub save_exact: Option<bool>,

    /// `savePeer` from `pnpm-workspace.yaml`. See
    /// [`Config::save_peer`]. Default `false`.
    ///
    /// [`Config::save_peer`]: crate::Config::save_peer
    pub save_peer: Option<bool>,

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

    /// `update` from `pnpm-workspace.yaml`. Supersedes `updateConfig`;
    /// see [`UpdateSettings`]. When both are set, `update` wins (with a
    /// warning) — the mapping onto [`Config::update_config`](crate::settings::Config::update_config) happens in
    /// [`Self::apply_to`].
    pub update: Option<UpdateSettings>,

    /// `updateConfig` from `pnpm-workspace.yaml`. See [`UpdateConfig`].
    ///
    /// Deprecated in favor of [`Self::update`], kept for backward
    /// compatibility until the next major version.
    pub update_config: Option<UpdateConfig>,

    /// `peerDependencyRules` from `pnpm-workspace.yaml`. See
    /// [`PeerDependencyRules`].
    pub peer_dependency_rules: Option<PeerDependencyRules>,

    /// `tasks` from `pnpm-workspace.yaml`: the workspace's task
    /// declarations, keyed by task (script) name. See [`TaskSettings`].
    pub tasks: Option<IndexMap<String, TaskSettings>>,

    /// `pipelines` from `pnpm-workspace.yaml`: named sets of task requests
    /// for `pnpm pipeline`, keyed by pipeline name. A pipeline is a set,
    /// not a sequence — ordering among its tasks is `tasks.dependsOn`'s
    /// job.
    pub pipelines: Option<IndexMap<String, Vec<String>>>,

    /// `pipelineBase` from `pnpm-workspace.yaml`: the git ref
    /// `pnpm pipeline` resolves its affected-selection merge base against.
    pub pipeline_base: Option<String>,

    /// The problem keys [`Self::collect_key_issues`] found in the file this
    /// was parsed from. Not a setting: carried here so the CLI can report
    /// them at the point where it knows how severe they are (see the
    /// warnings/error in `pnpm-cli`'s `config_warnings`).
    #[serde(skip)]
    pub key_issues: WorkspaceKeyIssues,
}

impl WorkspaceSettings {
    /// Read the global config.yaml at `<config_dir>/config.yaml`, if
    /// present.
    ///
    /// This file uses the same parser as `pnpm-workspace.yaml`, but a
    /// key-filter pass ([`Self::clear_workspace_only_fields`]) drops
    /// workspace-only knobs (`nodeLinker`, `hoist`, `lockfile`, ...)
    /// so they cannot be set globally.
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
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: path.clone(), source })?;
        settings.validate_registries()?;
        settings.validate_tasks()?;
        settings.validate_pipelines()?;
        settings.clear_workspace_only_fields();
        settings.warn_about_dropped_keys(&text, &path);
        Ok(Some(settings))
    }

    /// Warn about the keys of the global `config.yaml` that never reach the
    /// settings, in the three messages pnpm emits for that file.
    ///
    /// What survived is read back off `self` rather than off a second list of
    /// key names, which would drift from the struct: a key serde did not
    /// recognize is absent from the serialized settings, and one
    /// [`Self::clear_workspace_only_fields`] zeroed is null there.
    ///
    /// A dropped camelCase key pnpm's `isConfigFileKey` accepts stays silent:
    /// pnpm honors it in this file, so the fix is to honor it too, and until
    /// then a warning would diverge from pnpm's output on the same file.
    fn warn_about_dropped_keys(&self, text: &str, path: &Path) {
        let Ok(document) = serde_saphyr::from_str::<IndexMap<String, Option<IgnoredAny>>>(text)
        else {
            return;
        };
        let Ok(serde_json::Value::Object(kept)) = serde_json::to_value(self) else {
            return;
        };

        let mut dropped = DroppedKeys::default();
        for key in document.iter().filter(|(_, value)| value.is_some()).map(|(key, _)| key) {
            if key == SCHEMA_DIRECTIVE_KEY
                || matches!(kept.get(key), Some(value) if !value.is_null())
            {
                continue;
            }
            // The key comes from a file the machine's user controls, but the
            // same rendering serves the project file, so it is sanitized here
            // too rather than only where it must be.
            dropped.classify(&redact_and_sanitize(key));
        }
        dropped.warn(path);
    }

    /// Read `<dir>/pnpm-workspace.yaml` without walking ancestors.
    /// Returns `Ok(None)` only when nothing exists at that exact path;
    /// every other error (including `EISDIR` for a directory named
    /// `pnpm-workspace.yaml`, or permission denied) propagates, matching
    /// pnpm where `ENOENT` is the only silent case.
    pub fn load_at(dir: &Path) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        let path = dir.join(WORKSPACE_MANIFEST_FILENAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LoadWorkspaceYamlError::ReadFile { path, source }),
        };
        let mut settings: WorkspaceSettings = text
            .pipe_as_ref(serde_saphyr::from_str)
            .map_err(Box::new)
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: path.clone(), source })?;
        settings.validate_registries()?;
        settings.validate_tasks()?;
        settings.validate_pipelines()?;
        settings.reject_repo_controlled_trust_material(&path)?;
        settings.collect_key_issues(&text);
        Ok(Some(settings))
    }

    /// Walk up from `start_dir` looking for a readable `pnpm-workspace.yaml`.
    /// Returns `Ok(None)` if no ancestor has one. Per-level semantics are
    /// [`Self::load_at`]'s.
    pub fn find_and_load(
        start_dir: &Path,
    ) -> Result<Option<(PathBuf, Self)>, LoadWorkspaceYamlError> {
        for dir in start_dir.ancestors() {
            if let Some(settings) = Self::load_at(dir)? {
                return Ok(Some((dir.join(WORKSPACE_MANIFEST_FILENAME), settings)));
            }
        }
        Ok(None)
    }
}
