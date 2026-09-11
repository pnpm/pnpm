use super::{
    AuditConfig, AuditLevel, BTreeMap, BTreeSet, CalcPatchHashError, CargoSettings, CatalogMode,
    ColorMode, ConfigDependency, EnvVar, GlobalShims, HashMap, HoistingLimits, Host, IndexMap,
    InitType, LinkWorkspacePackages, NodeLinker, NodePackageMapType, PackageImportMethod,
    PackageManagerBootstrap, PatchGroupRecord, PatchInput, Path, PathBuf, Pipe, PmOnFail,
    ProjectConfig, PythonSettings, RegistryOptions, RemoteSideEffectsCacheSettings, ResolutionMode,
    ResolvePatchedDependenciesError, RuntimeOnFail, SaveWorkspaceProtocol, ScriptsPrependNodePath,
    SmartDefault, StoreDir, TrustPolicy, VerifyDepsBeforeRun, WorkspaceKeyIssues,
    create_hex_hash_from_file, default_cache_dir, default_child_concurrency,
    default_enable_global_virtual_store, default_fetch_min_speed_ki_bps, default_fetch_retries,
    default_fetch_retry_factor, default_fetch_retry_maxtimeout, default_fetch_retry_mintimeout,
    default_fetch_timeout, default_fetch_warn_timeout_ms, default_git_shallow_hosts,
    default_hoist_pattern, default_modules_cache_max_age, default_modules_dir,
    default_peers_suffix_max_length, default_public_hoist_pattern, default_registry,
    default_state_dir, default_store_dir, default_unsafe_perm, default_user_agent,
    default_virtual_store_dir, default_virtual_store_dir_max_length, default_workspace_concurrency,
    group_patched_dependencies, npmrc_auth, resolve_and_group, side_effects_cache_remote_env,
    workspace_yaml,
};

pub(super) fn default_ci<Sys: EnvVar>(detect_ci: fn() -> bool) -> bool {
    let ci = Sys::var("CI");
    if ci.as_deref() == Some("false") {
        return false;
    }

    matches!(ci.as_deref(), Some("true" | "1" | "woodpecker"))
        || Sys::var("GITHUB_ACTIONS").is_some()
        || detect_ci()
}

/// The two hoist patterns as one value, for
/// [`Config::hoist_patterns_before_virtual_store_only`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoistPatterns {
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
}

/// Resolved runtime config built from defaults, the auth subset of
/// `.npmrc`, and `pnpm-workspace.yaml` (see [`Config::current`]).
///
/// The type carries the merged result — it is never deserialized from a
/// file directly. Yaml is parsed into [`WorkspaceSettings`](crate::WorkspaceSettings) and applied
/// onto `Config` field-by-field, following pnpm 11's split between
/// `.npmrc` (auth/registry/network) and `pnpm-workspace.yaml`
/// (project-structural settings).
#[derive(Debug, Clone, SmartDefault)]
pub struct Config {
    /// Whether recursive commands stop after the first failure.
    #[default = true]
    pub bail: bool,

    /// Whether pnpm is running in a continuous-integration environment.
    /// Defaults to automatic CI detection and may be overridden through
    /// configuration.
    #[default(_code = "default_ci::<Host>(is_ci::cached)")]
    pub ci: bool,

    /// `updateNotifier` — whether `pnpm install` / `pnpm add` may check
    /// the registry once a day for a newer pnpm and print a notice when
    /// one exists. Setting it to `false` silences the check entirely.
    #[default = true]
    pub update_notifier: bool,

    /// ANSI color policy for human-readable output.
    pub color: ColorMode,

    /// Include a package's README in the generated manifest when packing.
    pub embed_readme: bool,

    /// Permit `add` to modify a multi-package workspace root without `-w`.
    pub ignore_workspace_root_check: bool,

    /// Include optional dependencies in install-family operations.
    #[default = true]
    pub optional: bool,

    /// npm-compatible alias used when `lockfile` was not set explicitly.
    #[default = true]
    pub package_lock: bool,

    /// Rebuild only dependencies and projects whose builds are pending.
    pub pending: bool,

    /// Make an unfiltered workspace install operate recursively.
    #[default = true]
    pub recursive_install: bool,

    /// Reverse the project order of recursive commands.
    pub reverse: bool,

    /// Stream a recursive command's script output as it arrives, one
    /// prefixed line at a time, instead of letting the child write to the
    /// terminal directly.
    pub stream: bool,

    /// Hold each script's streamed output until the script exits, then
    /// print it as one block. Only affects streamed output.
    pub aggregate_output: bool,

    /// Omit the project prefix from the streamed output lines of running
    /// scripts. `None` means the user never asked either way, which
    /// `exec` distinguishes from an explicit `false`.
    pub reporter_hide_prefix: Option<bool>,

    /// Route reporter output to stderr, leaving stdout for the command's
    /// own machine-readable result.
    pub use_stderr: bool,

    /// Treat the project as standalone: no workspace root is discovered,
    /// so `pnpm-workspace.yaml` contributes neither settings nor sibling
    /// projects.
    ///
    /// Only a caller that seeds this *before* [`Self::current`] — the
    /// `--ignore-workspace` flag — suppresses the search. pnpm resolves
    /// the workspace dir from argv alone (`getWorkspaceDir` in
    /// `config/reader/src/index.ts` reads the parsed CLI options, never
    /// the merged configuration), so a value arriving from a
    /// configuration file or `PNPM_CONFIG_IGNORE_WORKSPACE` lands here
    /// too late to affect discovery — deliberately, since a
    /// `pnpm-workspace.yaml` cannot coherently ask not to be read. Such
    /// a value still reaches the settings-only readers, matching pnpm's
    /// `handleIgnoredBuilds`.
    pub ignore_workspace: bool,

    /// Whether [`Self::current`] skipped workspace discovery because
    /// [`Self::ignore_workspace`] was seeded from `--ignore-workspace`.
    ///
    /// A consumer that re-derives the workspace root has to distinguish
    /// "no workspace dir was resolved" from "the search was deliberately
    /// suppressed", and cannot read [`Self::ignore_workspace`] to do it:
    /// by the time the load finishes, that boolean also carries values
    /// from `pnpm-workspace.yaml` and `PNPM_CONFIG_IGNORE_WORKSPACE`,
    /// which land too late to affect discovery and must not retroactively
    /// turn the project standalone.
    pub workspace_search_skipped: bool,

    /// Glob patterns selecting the workspace's projects, from
    /// `--workspace-packages` or `pnpm-workspace.yaml`'s `packages`.
    /// `None` outside a workspace.
    pub workspace_package_patterns: Option<Vec<String>>,

    /// Run lifecycle scripts through pnpm's portable shell emulator.
    pub shell_emulator: bool,

    /// Preserve publish-only manifest fields in packed manifests.
    pub skip_manifest_obfuscation: bool,

    /// Sort recursive workspace projects topologically.
    #[default = true]
    pub sort: bool,

    /// Select the beta CLI implementation when one is available.
    pub use_beta_cli: bool,

    /// The problem keys of the project's own `pnpm-workspace.yaml` (see
    /// [`WorkspaceKeyIssues`]), for the CLI to report. Empty when there is no
    /// workspace manifest or it is clean.
    pub workspace_key_issues: WorkspaceKeyIssues,

    /// When true, all dependencies are hoisted to `node_modules/.pnpm/node_modules`.
    /// This makes unlisted dependencies accessible to all packages inside `node_modules`.
    #[default = true]
    pub hoist: bool,

    /// Tells pnpm which packages should be hoisted to `node_modules/.pnpm/node_modules`.
    /// By default, all packages are hoisted - however, if you know that only some flawed packages
    /// have phantom dependencies, you can use this option to exclusively hoist the phantom
    /// dependencies (recommended).
    ///
    /// `None` corresponds to `null`: hoisting on the private side
    /// is disabled. `Some([])` is "feature on but no pattern matches",
    /// which still triggers the hoist pass (in case `public_hoist_pattern`
    /// is set). `Some(non-empty)` is the normal case. The default is
    /// `Some(["*"])`.
    ///
    /// The hoist guard at the install call site is
    /// `hoist_pattern.is_some() || public_hoist_pattern.is_some()`.
    #[default(_code = "Some(default_hoist_pattern())")]
    pub hoist_pattern: Option<Vec<String>>,

    /// Unlike hoist-pattern, which hoists dependencies to a hidden modules directory inside the
    /// virtual store, public-hoist-pattern hoists dependencies matching the pattern to the root
    /// modules directory. Hoisting to the root modules directory means that application code will
    /// have access to phantom dependencies, even if they modify the resolution strategy improperly.
    ///
    /// Same `Option` semantics as [`Self::hoist_pattern`] — `None`
    /// disables public hoisting, `Some([])` runs the hoist pass with
    /// no public matches, `Some(non-empty)` is the standard case.
    /// Default is `Some([])` (`'public-hoist-pattern': []`)
    /// — any non-empty default would write a `publicHoistPattern`
    /// into `.modules.yaml` that the next `pnpm` invocation rejects
    /// with `ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF`
    /// ([pnpm/pnpm#11750](https://github.com/pnpm/pnpm/issues/11750)).
    #[default(_code = "Some(default_public_hoist_pattern())")]
    pub public_hoist_pattern: Option<Vec<String>>,

    /// The patterns [`apply_virtual_store_only_derivation`] emptied, so
    /// a command-line `--no-virtual-store-only` that outranks a lower
    /// layer's `virtualStoreOnly: true` can bring them back exactly.
    /// `None` until that derivation empties them.
    ///
    /// [`apply_virtual_store_only_derivation`]: Self::apply_virtual_store_only_derivation
    pub hoist_patterns_before_virtual_store_only: Option<HoistPatterns>,

    /// `extendNodePath`: when `true` (the default) and the isolated
    /// `nodeLinker` runs with a hoist pattern, command shims set
    /// `NODE_PATH` to include the hidden hoisted modules directory
    /// (`<virtual-store-dir>/node_modules`). `false` leaves `NODE_PATH`
    /// out of the shims entirely.
    #[default(true)]
    pub extend_node_path: bool,

    /// `preferSymlinkedExecutables`: on Unix, link `node_modules/.bin`
    /// entries as plain symlinks to the target bin file instead of
    /// writing shell shims. Symlinked bins have no shim to carry a
    /// `NODE_PATH` block, so [`Config::current`] compensates by
    /// exporting `NODE_PATH=<virtual-store-dir>/node_modules` to every
    /// spawned child process. Inert on Windows, where bins always get
    /// shims.
    ///
    /// `None` — the default — means "not configured": the hoisted
    /// `nodeLinker` then turns it on (see
    /// [`Self::apply_prefer_symlinked_executables_derivation`]), which
    /// an explicit `false` prevents.
    pub prefer_symlinked_executables: Option<bool>,

    /// By default, pnpm creates a semistrict `node_modules`, meaning dependencies have access to
    /// undeclared dependencies but modules outside of `node_modules` do not. With this layout,
    /// most of the packages in the ecosystem work with no issues. However, if some tooling only
    /// works when the hoisted dependencies are in the root of `node_modules`, you can set this to
    /// true to hoist them for you.
    pub shamefully_hoist: bool,

    /// The location where all packages are saved on disk. Share a
    /// writable store only between mutually trusted users, jobs, and
    /// processes.
    #[default(_code = "default_store_dir::<Host>()")]
    pub store_dir: StoreDir,

    /// The machine-local directory in which pnpm persists state across
    /// invocations. A project's manifest cannot set this path.
    #[default(_code = "default_state_dir::<Host>().unwrap_or_default()")]
    pub state_dir: PathBuf,

    /// The directory in which dependencies will be installed (instead of `node_modules`).
    #[default(_code = "default_modules_dir()")]
    pub modules_dir: PathBuf,

    /// Defines what linker should be used for installing Node packages.
    pub node_linker: NodeLinker,

    /// When true, pacquet writes `node_modules/.package-map.json` for
    /// Node's `--experimental-package-map` loader flag. Default
    /// `false`, matching pnpm's opt-in setting.
    pub node_experimental_package_map: bool,

    /// Selects the package-map dependency surface. Pacquet currently
    /// materializes only the standard map for isolated installs; loose
    /// and hoisted maps require layout-aware writers.
    pub node_package_map_type: NodePackageMapType,

    /// When symlink is set to false, pnpm creates a virtual store directory without any symlinks.
    /// It is a useful setting together with node-linker=pnp.
    #[default = true]
    pub symlink: bool,

    /// The directory with links to the store. All direct and indirect dependencies of the
    /// project are linked into this directory.
    ///
    /// When [`enable_global_virtual_store`] is `true` and the user has not
    /// explicitly set this field, [`Config::current`] re-points it at
    /// `<store_dir>/v11/links`. The `v11/` segment comes from appending
    /// `STORE_VERSION` to the configured `storeDir` before the
    /// `join(storeDir, 'links')` step runs — so the join lands one level
    /// deeper than the configured root.
    ///
    /// [`enable_global_virtual_store`]: Self::enable_global_virtual_store
    #[default(_code = "default_virtual_store_dir()")]
    pub virtual_store_dir: PathBuf,

    /// When `true`, the virtual store is shared across every project on
    /// the machine: packages live under `<store_dir>/v11/links/...` and
    /// each project registers itself at
    /// `<store_dir>/v11/projects/<short-hash>`. When `false`, each
    /// project keeps its own virtual store at
    /// `<project>/node_modules/.pnpm`.
    ///
    /// Defaults to `false`, matching the TypeScript CLI.
    #[default(_code = "default_enable_global_virtual_store()")]
    pub enable_global_virtual_store: bool,

    /// The shared global-virtual-store directory. When
    /// [`enable_global_virtual_store`] is `true` this is the same path as
    /// [`virtual_store_dir`]; when `false`, it is still computed as
    /// `<store_dir>/v11/links` (an unconditional assignment) even though
    /// no install path consults it in that mode today.
    ///
    /// Populated by [`Config::current`] after yaml has been applied; the
    /// `SmartDefault` value is overwritten there with the path derived
    /// from the resolved `store_dir` / `virtual_store_dir`. The default
    /// here is only meaningful when `Config::new()` is used in isolation
    /// (mostly tests), and matches the derivation's own fallback so
    /// such a config never points the shared store at the working
    /// directory.
    ///
    /// [`enable_global_virtual_store`]: Self::enable_global_virtual_store
    /// [`virtual_store_dir`]: Self::virtual_store_dir
    #[default(_code = "default_store_dir::<Host>().links()")]
    pub global_virtual_store_dir: PathBuf,

    /// `virtualStoreOnly`: populate the virtual store but perform no
    /// post-import linking — no importer symlinks, no `.bin` entries,
    /// no hoisting, and no project lifecycle scripts. `pnpm fetch` is
    /// the canonical consumer.
    ///
    /// [`Self::apply_virtual_store_only_derivation`] clears both hoist
    /// patterns when this is set. Combining it with
    /// `enable_modules_dir: false` while the global virtual store is
    /// off is a config conflict, rejected by
    /// `pnpm_package_manager::Install::run`.
    pub virtual_store_only: bool,

    /// `enableModulesDir`: pnpm's setting for suppressing the
    /// `node_modules` directory entirely. Default `true`.
    ///
    /// A `false` value (with the global virtual store off) makes the
    /// install "resolve and write the lockfile, materialize nothing" —
    /// it rides the `--lockfile-only` pipeline in
    /// `pnpm_package_manager::Install::run`. With the global virtual
    /// store on, materialization proceeds into the store (pnpm's
    /// `enableModulesDir !== false || enableGlobalVirtualStore` gate).
    /// It also gates the [`virtual_store_only`] config conflict (a
    /// store-only install with no modules dir needs the global virtual
    /// store to have anywhere to put packages).
    ///
    /// [`virtual_store_only`]: Self::virtual_store_only
    #[default(true)]
    pub enable_modules_dir: bool,

    /// User override for the global packages root (`global-dir` setting /
    /// `PNPM_CONFIG_GLOBAL_DIR`). When unset, [`Config::current`] derives
    /// the root from the pnpm home directory.
    pub global_dir: Option<PathBuf>,

    /// User override for the global bin directory (`global-bin-dir` setting
    /// / `PNPM_CONFIG_GLOBAL_BIN_DIR`). When unset, [`Config::current`]
    /// derives it as `<pnpm-home>/bin`.
    pub global_bin_dir: Option<PathBuf>,

    /// The resolved global packages directory,
    /// `(global_dir ?? <pnpm-home>/global)/v11`. Populated by
    /// [`Config::current`]; `None` when the pnpm home directory cannot be
    /// determined and no override is set.
    pub global_pkg_dir: Option<PathBuf>,

    /// The resolved global bin directory, `global_bin_dir ?? <pnpm-home>/bin`.
    /// Populated by [`Config::current`]; global add/remove/update require it
    /// (pnpm's `NO_GLOBAL_BIN_DIR` when absent).
    pub global_bin: Option<PathBuf>,

    /// `globalShims`, resolved: which globally installed
    /// packages get context-aware shims and under which trust policy,
    /// keyed by package name and merged key-wise across the
    /// configuration layers over the built-in
    /// `{ node: true, deno: true, bun: true }`. See
    /// [`GlobalShims`].
    pub global_shims: GlobalShims,

    /// Controls the way packages are imported from the store (if you want to disable symlinks
    /// inside `node_modules`, then you need to change the node-linker setting, not this one).
    pub package_import_method: PackageImportMethod,

    /// The time in minutes after which orphan packages from the modules directory should be
    /// removed. pnpm keeps a cache of packages in the modules directory. This boosts installation
    /// speed when switching branches or downgrading dependencies.
    ///
    /// Default value is 10080 (7 days in minutes)
    #[default(_code = "default_modules_cache_max_age()")]
    pub modules_cache_max_age: u64,

    /// Maximum filename length for the per-snapshot subdirectory of the
    /// virtual store (`node_modules/.pnpm/<name>`). When the escaped
    /// flat name would exceed this many bytes, the tail is replaced
    /// with a 32-char sha256 hash so the path stays within filesystem
    /// limits (macOS / ext4 cap component names at 255 bytes; pnpm
    /// defaults to 60 on Windows and 120 elsewhere to leave headroom
    /// for `node_modules/<name>` suffixes appended below).
    ///
    /// Configurable via `virtualStoreDirMaxLength` in
    /// `pnpm-workspace.yaml`, global `config.yaml`, or
    /// `PNPM_CONFIG_VIRTUAL_STORE_DIR_MAX_LENGTH`. The same value is
    /// persisted into `node_modules/.modules.yaml` so subsequent
    /// installs see the user's pick.
    ///
    /// Default value is 60 on Windows and 120 otherwise.
    #[default(_code = "default_virtual_store_dir_max_length()")]
    pub virtual_store_dir_max_length: u64,

    /// Cap on the rendered peer-suffix length before the suffix is
    /// replaced with a short hash. Threaded into
    /// `pnpm_deps_path::create_peer_dep_graph_hash` — when the
    /// flattened `(peer@ver)(peer@ver)…` string exceeds this many
    /// bytes, pacquet swaps it for a 32-char sha256 hash so
    /// virtual-store paths stay under the OS component-name limit.
    ///
    /// Configurable via `peersSuffixMaxLength` in
    /// `pnpm-workspace.yaml`, global `config.yaml`, or
    /// `PNPM_CONFIG_PEERS_SUFFIX_MAX_LENGTH`. The same value is
    /// persisted into the lockfile's `settings.peersSuffixMaxLength`
    /// (omitted when it equals the default) so subsequent installs
    /// pick the user's pick.
    ///
    /// Default value is 1000.
    #[default(_code = "default_peers_suffix_max_length()")]
    pub peers_suffix_max_length: u64,

    /// When set to false, pnpm won't read or generate a pnpm-lock.yaml file.
    ///
    /// Defaults to `true` so a fresh `pacquet install` writes a
    /// lockfile by default.
    #[default = true]
    pub lockfile: bool,

    /// Where `pnpm-lock.yaml` is read and written, when the user pins it
    /// with the `lockfileDir` setting (or `--lockfile-dir`). Several
    /// projects may share one lockfile this way. Absolute once
    /// [`Config::current`] has resolved it; `None` means "derive it",
    /// which [`Config::lockfile_dir_for`] does.
    ///
    /// Every path anchored on the lockfile — the root `node_modules`, the
    /// virtual store, and the importer ids — follows it, so setting it
    /// goes through [`Config::pin_lockfile_dir`].
    pub lockfile_dir: Option<PathBuf>,

    /// When set to true and the available pnpm-lock.yaml satisfies the package.json dependencies
    /// directive, a headless installation is performed. A headless installation skips all
    /// dependency resolution as it does not need to modify the lockfile.
    #[default = true]
    pub prefer_frozen_lockfile: bool,

    /// The `frozenLockfile` setting: `install` neither re-resolves nor
    /// writes `pnpm-lock.yaml`, and fails when the lockfile is out of
    /// date with the manifests.
    ///
    /// `None` — the default — means "not configured", which the CLI
    /// distinguishes from an explicit `false` (`--no-frozen-lockfile`)
    /// so the two can layer over each other in the usual
    /// CLI-beats-config order.
    pub frozen_lockfile: Option<bool>,

    /// When `true`, `pacquet install` performs a workspace-state
    /// freshness check before any of the install setup runs and
    /// returns immediately ("Already up to date") if nothing has
    /// changed since the previous install.
    ///
    /// The `optimisticRepeatInstall` setting. The fast path keys off
    /// `.pnpm-workspace-state-v1.json`'s `lastValidatedTimestamp` vs
    /// each project's `package.json` mtime, so it never reads the
    /// lockfile or the verifier cache when no manifest has been touched.
    ///
    /// Defaults to `true`.
    #[default = true]
    pub optimistic_repeat_install: bool,

    /// When `true`, runtime dependencies (`node@runtime:`,
    /// `deno@runtime:`, `bun@runtime:`) are skipped at install
    /// time — their archives aren't fetched, their slots aren't
    /// materialized, and their bins aren't linked. The rest of
    /// the install proceeds normally. The `skipRuntimes` option,
    /// exposed via the `--no-runtime` CLI flag.
    ///
    /// Defaults to `false`. CI scenarios that
    /// pre-provision the runtime (or want to install one runtime
    /// with another pacquet binary) flip this to `true`.
    pub skip_runtimes: bool,

    /// When `true`, a dependency whose `engines` (or `cpu` / `os` / `libc`)
    /// constraint the host does not satisfy fails the install with
    /// `ERR_PNPM_UNSUPPORTED_ENGINE` instead of being skipped (optional) or
    /// warned about (required). The `engineStrict` setting; default `false`,
    /// matching pnpm.
    pub engine_strict: bool,

    /// Overrides the Node.js version used as the `engines.node` satisfiability
    /// target for the installability check. The `nodeVersion` setting. When
    /// `None` (the default), the version is auto-detected from the `node`
    /// binary on `PATH` (falling back to a synthetic high version when no
    /// `node` is found). An explicit value is treated as authoritative — no
    /// `node --version` probe runs.
    pub node_version: Option<String>,

    /// Override for `devEngines.runtime.onFail` / `engines.runtime.onFail`.
    /// Unset by default so each manifest keeps its own policy.
    pub runtime_on_fail: Option<RuntimeOnFail>,

    /// Per-release-channel Node.js download mirrors. Keys are `release`,
    /// `rc`, `nightly`, `test`, or `v8-canary`.
    pub node_download_mirrors: HashMap<String, String>,

    /// Copy every project file during `pnpm deploy` instead of the publish
    /// packlist. The `deployAllFiles` setting; default `false`.
    pub deploy_all_files: bool,

    /// Force `pnpm deploy` to use the legacy install-based implementation
    /// even when a shared workspace lockfile is available.
    pub force_legacy_deploy: bool,

    /// Whether the workspace uses a single root `pnpm-lock.yaml`. The
    /// `sharedWorkspaceLockfile` setting; default `true`.
    #[default = true]
    pub shared_workspace_lockfile: bool,

    /// `gitBranchLockfile` — give each git branch its own
    /// `pnpm-lock.<branch>.yaml` instead of sharing `pnpm-lock.yaml`, so
    /// two branches can hold different resolutions without conflicting on
    /// one file. Default `false`.
    ///
    /// The name the install actually reads and writes is
    /// [`Self::git_branch_lockfile_name`]; this flag alone does not decide
    /// it, because the branch may be unknown and
    /// [`Self::merge_git_branch_lockfiles`] overrides it.
    pub use_git_branch_lockfile: bool,

    /// `mergeGitBranchLockfiles` — fold every `pnpm-lock.<branch>.yaml`
    /// into `pnpm-lock.yaml` and delete them, which is what a branch's
    /// merge back into the mainline needs. Default `false`, or whatever
    /// [`Self::merge_git_branch_lockfiles_branch_pattern`] decides for the
    /// current branch.
    pub merge_git_branch_lockfiles: bool,

    /// `mergeGitBranchLockfilesBranchPattern` — glob patterns naming the
    /// branches that merge the per-branch lockfiles, so the mainline
    /// branches need not pass `--merge-git-branch-lockfiles` by hand.
    /// Consulted only when `mergeGitBranchLockfiles` is not set outright.
    pub merge_git_branch_lockfiles_branch_pattern: Vec<String>,

    /// The `pnpm-lock.<branch>.yaml` the current git branch resolves to
    /// under [`Self::use_git_branch_lockfile`]. `None` when the setting is
    /// off or the branch cannot be determined (a detached HEAD, or no
    /// repository at all), in which case the install stays on
    /// `pnpm-lock.yaml`.
    pub git_branch_lockfile_name: Option<String>,

    /// Refuse network requests during install. The `offline` flag gates
    /// the metadata-fetch path with `ERR_PNPM_NO_OFFLINE_META` when no
    /// cached metadata exists for a spec. Pacquet doesn't have a
    /// metadata-fetch path yet (no resolver until Stage 2), so the same
    /// flag instead gates pacquet's tarball-fetch fall-through: when both
    /// the warm prefetch and the `SQLite` `index.db` lookup miss, the
    /// tarball fetcher fails fast with `ERR_PNPM_NO_OFFLINE_TARBALL`
    /// rather than hitting the registry. The frozen-lockfile install
    /// path needs no metadata, so the surface area collapses to
    /// "every snapshot must already be in the local store".
    ///
    /// Pacquet's tarball-side gate has no exact pnpm counterpart
    /// (pnpm doesn't gate the tarball fetcher on `offline`), but it's
    /// the most useful interpretation of the flag for a frozen
    /// installer: surface a clear `offline` error rather than letting
    /// the underlying `connection refused` / DNS error propagate.
    /// The Stage 2 resolver will additionally honor the flag on the
    /// metadata path.
    pub offline: bool,

    /// Prefer the local store on read, fall back to the network on a
    /// cache miss. The `preferOffline` flag biases the resolver to use
    /// cached metadata when available even past the freshness window.
    ///
    /// Pacquet's frozen-install path already prefers the local store
    /// — the warm prefetch + SQLite-cache lookups always run before
    /// any network fetch — so `prefer_offline` is effectively a no-op
    /// today. The field exists so `.npmrc` / yaml / CLI all parse the
    /// flag cleanly; Stage 2's resolver will honor it.
    pub prefer_offline: bool,

    /// Add the full URL to the package's tarball to every entry in pnpm-lock.yaml.
    pub lockfile_include_tarball_url: bool,

    /// The base URL of the npm package registry (trailing slash included).
    #[default(_code = "default_registry()")]
    pub registry: String, // TODO: use Url type (compatible with reqwest)

    /// The default package scope for `pnpm login` and `pnpm adduser`: the
    /// granted token is associated with this scope and the scope-to-registry
    /// mapping is recorded. Overridden by `--scope`.
    ///
    /// No repo-committed config file can set it — see
    /// [`crate::refused_keys`].
    pub scope: Option<String>,

    /// Scoped registry routes keyed by `@scope`, populated from
    /// `.npmrc` `@scope:registry=...` and the scopes a
    /// `pnpm-workspace.yaml#registries` entry declares.
    pub registries_by_scope: BTreeMap<String, String>,

    /// User-defined named-registry aliases from
    /// `pnpm-workspace.yaml#namedRegistries`. Maps each alias name
    /// (`gh`, `work`, ...) to the registry URL its `<alias>:` specifiers
    /// resolve against. Empty by default — the resolver layer merges
    /// these on top of pnpm's built-in defaults (today: `gh:` →
    /// GitHub Packages) and rejects malformed URLs at construction
    /// time with `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    ///
    /// The `prefix` a `registries` entry declares, or the deprecated
    /// `namedRegistries` setting.
    pub registries_by_prefix: BTreeMap<String, String>,

    /// Non-secret per-registry settings from
    /// `pnpm-workspace.yaml#registries`, keyed by registry URL with a
    /// trailing slash. Deliberately separate from the auth config: that one
    /// carries credentials, and the install and lockfile layers that need a
    /// registry's tarball layout must not be handed its secrets.
    ///
    /// The `registries` setting.
    pub registry_options_by_url: BTreeMap<String, RegistryOptions>,

    /// Resolved proxy configuration — `https-proxy`, `http-proxy`, and
    /// `no-proxy` (plus the legacy `proxy` key and env-var fallbacks),
    /// all from `.npmrc` and the process environment. The type lives
    /// in `pnpm-network` (where it is consumed by
    /// `ThrottledClient::for_installs`) because `pnpm-config`
    /// already depends on `pnpm-network` for auth-headers plumbing.
    /// Default is empty (`None` for every field) — i.e. no proxy.
    pub proxy: pnpm_network::ProxyConfig,

    /// Every proxy key as written, merged across config layers.
    /// [`Self::proxy`] is its resolution — see [`crate::proxy_keys`].
    pub proxy_keys: crate::proxy_keys::ProxyKeys,

    /// Resolved TLS + `local-address` configuration — `ca`, `cafile`,
    /// `cert`, `key`, `strict-ssl`, `local-address` from `.npmrc`. The
    /// type lives in `pnpm-network` for the same reason as
    /// [`Self::proxy`]. `strict_ssl: None` here means "unset"; the
    /// `true` default is applied at client-build time by
    /// `ThrottledClient::for_installs` (`strictSsl ?? true`).
    pub tls: pnpm_network::TlsConfig,

    /// Per-registry TLS overrides — `//host[:port]/path/:ca`,
    /// `:cafile`, `:cert`, `:certfile`, `:key`, `:keyfile` from
    /// `.npmrc`. Lookup uses pnpm's 5-step nerf-darted fallback
    /// chain (exact > nerf-dart > no-port > shorter path prefix >
    /// recursive no-port retry). Per-registry fields override
    /// [`Self::tls`] field-by-field at request time (a
    /// `{ ...opts, ...sslConfig }` spread).
    pub tls_by_uri: pnpm_network::PerRegistryTls,

    /// When true, any missing non-optional peer dependencies are automatically installed.
    #[default = true]
    pub auto_install_peers: bool,

    /// When `true`, dependencies declared with the `link:` protocol
    /// are excluded from `pnpm-lock.yaml`. Workspace-protocol
    /// dependencies (`workspace:`), which also resolve to a link,
    /// are still recorded. The `excludeLinksFromLockfile` setting
    /// (default `false`).
    pub exclude_links_from_lockfile: bool,

    /// When `true`, conflicting peer-dependency ranges from multiple
    /// consumers are merged with `||` (so the resolver may pick the
    /// highest version that satisfies any one of them) instead of
    /// being dropped when their intersection is empty. The
    /// `autoInstallPeersFromHighestMatch` setting.
    pub auto_install_peers_from_highest_match: bool,

    /// The `hoistWorkspacePackages` setting. When `true` (the
    /// default, matching pnpm), each named workspace project is
    /// itself considered for hoisting: its name becomes a
    /// lowest-precedence root-level alias, and where a hoist pattern
    /// matches, `<hoisted modules dir>/<name>` symlinks straight to
    /// the project directory — so tooling resolving from the hoisted
    /// tree can `require` workspace packages by name.
    ///
    /// This knob never affects hoister-tree *membership*: non-root
    /// importers always participate in the shared hoist plan (v11
    /// semantics), so cross-project version dedupe is unconditional.
    #[default = true]
    pub hoist_workspace_packages: bool,

    /// Per-importer block-list of package aliases that may NOT be
    /// hoisted past that importer's slot. Outer key is the
    /// importer locator (e.g. `'.@'` for the root project, or the
    /// `hoistingLimits` from `pnpm-workspace.yaml`. Controls how far
    /// dependencies are hoisted under `nodeLinker: hoisted`. See
    /// [`HoistingLimits`] for the `none` / `workspaces` /
    /// `dependencies` semantics. Default [`HoistingLimits::None`]
    /// (hoist as far as possible). Translated into the hoister's
    /// per-locator border map by `crate::get_hoisting_limits` in
    /// `pnpm-package-manager`. No effect under
    /// `nodeLinker: isolated`.
    pub hoisting_limits: HoistingLimits,

    /// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Controls
    /// whether the npm resolver consults the workspace map when
    /// resolving bare-semver wanted dependencies. See
    /// [`LinkWorkspacePackages`] for the tri-state semantics.
    /// Default `false` (`'link-workspace-packages': false`).
    pub link_workspace_packages: LinkWorkspacePackages,

    /// `saveWorkspaceProtocol`. How `pacquet update --workspace`
    /// writes a dependency it links to a workspace package. See
    /// [`SaveWorkspaceProtocol`].
    /// Default `"rolling"` (`'save-workspace-protocol': 'rolling'`).
    pub save_workspace_protocol: SaveWorkspaceProtocol,

    /// `injectWorkspacePackages` from `pnpm-workspace.yaml`. When
    /// `true`, workspace-package resolutions materialize as `file:`
    /// (hard-linked copies into the virtual store) instead of `link:`
    /// symlinks back to the source. Per-dependency
    /// `dependenciesMeta[*].injected = true` opts a single dep into
    /// the same behavior even when this flag is `false`.
    ///
    /// Default `false` (`'inject-workspace-packages': undefined`).
    pub inject_workspace_packages: bool,

    /// When `true`, prefer a workspace package over a registry pick
    /// even when the registry version is newer than the workspace
    /// one. The `preferWorkspacePackages` setting, consumed by the npm
    /// resolver's registry-pick + workspace shadow.
    /// Default `false` (`'prefer-workspace-packages': false`).
    pub prefer_workspace_packages: bool,

    /// Name slots reserved at the root for an external linker
    /// (the Bit CLI is the only known consumer). Any dependency whose
    /// alias matches one of these names is stripped from the hoist
    /// tree's top-level entries — the external linker materializes
    /// those slots itself.
    ///
    /// Programmatic-only in pnpm; pacquet exposes the same yaml
    /// shape (`externalDependencies: ["bit-bin"]`).
    ///
    /// Default empty. No effect under `nodeLinker: isolated`.
    pub external_dependencies: BTreeSet<String>,

    /// When this setting is set to true, packages with peer dependencies will be deduplicated after peers resolution.
    #[default = true]
    pub dedupe_peer_dependents: bool,

    /// When `true`, peer-dependency suffixes in `depPath`s use
    /// version-only identifiers (`name@version`) instead of recursive
    /// dep paths, eliminating nested suffixes like
    /// `(foo@1.0.0(bar@2.0.0))`. The `dedupePeers` setting;
    /// default `false`.
    pub dedupe_peers: bool,

    /// When `true`, a direct dependency of a non-root workspace
    /// project is omitted from that project's `node_modules/` when
    /// the workspace root resolves the same alias to the same target.
    /// Drives both the linking step (which skips writing the
    /// per-importer symlink) and bin linking (the deduped dep won't
    /// reappear under the project's `node_modules/.bin`).
    ///
    /// Default `false` (`'dedupe-direct-deps': false`).
    #[default = false]
    pub dedupe_direct_deps: bool,

    /// When `true`, injected workspace dependencies whose materialised
    /// children turn out to be a subset of the target workspace
    /// project's own direct dependencies get rewritten back to
    /// symlinks. The `dedupeInjectedDeps` setting; default `true`.
    #[default = true]
    pub dedupe_injected_deps: bool,

    /// If this is enabled, commands will fail if there is a missing or invalid peer dependency in the tree.
    pub strict_peer_dependencies: bool,

    /// When true, skip pnpm's built-in compatibility database from
    /// `@yarnpkg/extensions`. Default `false` so known broken package
    /// manifests are patched during resolution.
    pub ignore_compatibility_db: bool,

    /// When enabled, dependencies of the root workspace project are used to resolve peer
    /// dependencies of any projects in the workspace. It is a useful feature as you can install
    /// your peer dependencies only in the root of the workspace, and you can be sure that all
    /// projects in the workspace use the same versions of the peer dependencies.
    #[default = true]
    pub resolve_peers_from_workspace_root: bool,

    /// When `true`, reject exotic (git, tarball, file, ...) dependencies
    /// reached transitively from the importer. Direct deps remain
    /// allowed. The `blockExoticSubdeps` setting; default `true`.
    #[default = true]
    pub block_exotic_subdeps: bool,

    /// Whether to verify each CAFS file's on-disk integrity before reusing it
    /// for an install. When `true` (pnpm's default), the store-index cache
    /// lookup stats each referenced file and re-hashes any whose mtime has
    /// advanced past the stored `checkedAt` timestamp. When `false`, the
    /// lookup skips that verification entirely and trusts the index — a
    /// missing blob is discovered lazily at link time instead.
    ///
    /// This is corruption detection for a trusted store, not a tamper
    /// boundary for a store writable by untrusted users or jobs.
    ///
    /// The `verifyStoreIntegrity` camelCase key in
    /// `pnpm-workspace.yaml` (default `true`).
    #[default = true]
    pub verify_store_integrity: bool,

    /// Whether a store row whose bundled `package.json` names a
    /// different package than the row was recorded for fails the
    /// install. When `true` (pnpm's default) the read raises
    /// `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE`; when `false` the row
    /// is used and the disagreement is only warned about.
    ///
    /// A lockfile that pairs an integrity with the wrong package, and a
    /// registry (or proxy) serving a tarball that does not match the
    /// metadata it was listed under, both surface here.
    ///
    /// The `strictStorePkgContentCheck` camelCase key in
    /// `pnpm-workspace.yaml` (default `true`).
    #[default = true]
    pub strict_store_pkg_content_check: bool,

    /// Opt-in assertion that the package store is complete and will not
    /// be written during this install — for running against a store on a
    /// read-only filesystem (a Nix store, a read-only bind mount, an OCI
    /// layer). When `true`, pacquet opens `index.db` through the
    /// `immutable=1` URI (see `StoreIndex::open_immutable`) and suppresses
    /// every store-write path: the batched `index.db` writer is replaced
    /// with a drain-and-drop stub that never opens the DB, and
    /// `init_store_dir_best_effort` is skipped so no directory creation is
    /// attempted under the store root. Pair with `--offline
    /// --frozen-lockfile` against a fully-populated store.
    ///
    /// pnpm rejects `frozenStore` combined with `force` (force re-imports
    /// packages into the store, which a read-only store cannot accept).
    /// The guard lives in the install pipeline's entry
    /// (`ERR_PNPM_CONFIG_CONFLICT_FROZEN_STORE_WITH_FORCE`); see
    /// [`Config::force`].
    ///
    /// The `frozenStore` / `--frozen-store` setting (default `false`).
    pub frozen_store: bool,

    /// pnpm's `--force`. Install every package the lockfile names, even
    /// ones whose `cpu` / `os` / `libc` / `engines` don't match the host
    /// — the per-snapshot installability check is bypassed entirely, so
    /// optional dependencies for foreign platforms are materialized
    /// instead of skipped, mirroring pnpm's `!opts.force &&
    /// packageIsInstallable(...)` gate in its dep-graph builders.
    ///
    /// CLI-only (merged from `--force` on `pnpm install` / `pnpm add` /
    /// `pnpm deploy` at the dispatch, like `ignoreScripts`); not a
    /// `pnpm-workspace.yaml` / `.npmrc` setting. On the frozen path it
    /// also discards the previous install's per-snapshot skip decision,
    /// mirroring pnpm's `lockfileToDepGraph(…, opts.force ? null :
    /// currentLockfile)`, so already-materialized packages are relinked.
    pub force: bool,

    /// Whether to consult the side-effects cache
    /// (`PackageFilesIndex.sideEffects`) when importing a package
    /// and whether to populate it after a successful postinstall.
    /// Read from `pnpm-workspace.yaml`'s `sideEffectsCache` field
    /// (camelCase, optional, defaults `true`).
    ///
    /// Default `true` (`side-effects-cache`).
    ///
    /// The READ gate combines this with [`side_effects_cache_readonly`]
    /// via [`Config::side_effects_cache_read`]; the WRITE gate via
    /// [`Config::side_effects_cache_write`]. Consume those helpers
    /// rather than reading this field directly so the precedence
    /// stays single-sourced.
    ///
    /// [`side_effects_cache_readonly`]: Self::side_effects_cache_readonly
    #[default = true]
    pub side_effects_cache: bool,

    /// Treat the side-effects cache as read-only — pacquet still
    /// honors cache hits on the READ side but does not populate
    /// the cache after a successful postinstall. The
    /// `side-effects-cache-readonly` setting; default `false`. Read
    /// from `pnpm-workspace.yaml`'s `sideEffectsCacheReadonly` field.
    ///
    /// Consume via [`Config::side_effects_cache_read`] and
    /// [`Config::side_effects_cache_write`].
    pub side_effects_cache_readonly: bool,

    /// How many times pacquet retries a failed tarball fetch on transient
    /// errors before giving up. The `fetchRetries` setting (default `2`).
    /// The value is the count of *retries*, so total attempts =
    /// `fetch_retries + 1`.
    ///
    /// Today this only gates the `pnpm-tarball` download path;
    /// `crates/registry`'s metadata fetches still issue a single request.
    /// Threading the same retry policy through the registry client is a
    /// follow-up.
    ///
    /// Read from `pnpm-workspace.yaml` only — pnpm 11 excludes the
    /// `fetch-retry*` family from `NPM_AUTH_SETTINGS`, so a
    /// `fetch-retries=…` line in `.npmrc` is ignored both there and here.
    #[default(_code = "default_fetch_retries()")]
    pub fetch_retries: u32,

    /// Exponential-backoff growth factor between retry attempts. The
    /// `fetchRetryFactor` setting (default `10`). Successive backoff is
    /// `min(fetch_retry_mintimeout * factor^attempt, fetch_retry_maxtimeout)`.
    /// Yaml-only — see [`Config::fetch_retries`].
    #[default(_code = "default_fetch_retry_factor()")]
    pub fetch_retry_factor: u32,

    /// Floor in milliseconds for the wait between retries. The
    /// `fetchRetryMintimeout` setting (default `10000` — 10 s). Yaml-only
    /// — see [`Config::fetch_retries`].
    #[default(_code = "default_fetch_retry_mintimeout()")]
    pub fetch_retry_mintimeout: u64,

    /// Cap in milliseconds on the wait between retries. The
    /// `fetchRetryMaxtimeout` setting (default `60000` — 1 min). Yaml-only
    /// — see [`Config::fetch_retries`].
    #[default(_code = "default_fetch_retry_maxtimeout()")]
    pub fetch_retry_maxtimeout: u64,

    /// Maximum number of concurrent network requests pacquet keeps
    /// in flight during install — the size of the [`pnpm_network`]
    /// semaphore. The `networkConcurrency` setting; the default is the
    /// `Math.min(96, Math.max(calcMaxWorkers() * 3, 64))` formula,
    /// implemented by [`pnpm_network::default_network_concurrency`].
    #[default(_code = "pnpm_network::default_network_concurrency()")]
    pub network_concurrency: usize,

    /// Maximum number of concurrent connections (sockets) to a single
    /// registry origin — the `maxSockets` setting, mirroring undici's
    /// per-origin `connections` cap that pnpm applies. `None` (the default)
    /// leaves the per-origin socket count bounded only by
    /// [`Self::network_concurrency`]; `Some(n)` additionally caps each
    /// `scheme://host[:port]` at `n` in-flight sockets, queueing the rest.
    pub max_sockets: Option<usize>,

    /// How long a request may make no progress, in milliseconds. The
    /// `fetchTimeout` setting (default `60000` — 60 s, see
    /// [`pnpm_network::DEFAULT_FETCH_TIMEOUT_MS`]). Applied as both the
    /// read and connect deadline of the reqwest client.
    #[default(_code = "default_fetch_timeout()")]
    pub fetch_timeout: u64,

    /// Successful registry metadata requests slower than this threshold emit
    /// a warning. The `fetchWarnTimeoutMs` setting, in milliseconds (default
    /// `10000`, or 10 s).
    #[default(_code = "default_fetch_warn_timeout_ms()")]
    pub fetch_warn_timeout_ms: u64,

    /// Minimum expected average tarball download speed in KiB/s. A download
    /// lasting more than one second warns when its average falls below this
    /// value. The `fetchMinSpeedKiBps` setting (default `50`).
    #[default(_code = "default_fetch_min_speed_ki_bps()")]
    pub fetch_min_speed_ki_bps: u64,

    /// Value of the `User-Agent` header sent on every registry request.
    /// The `userAgent` setting; the default is the
    /// `pnpm/<version> npm/? node/? <platform> <arch>` format (built by
    /// `default_user_agent`).
    #[default(_code = "default_user_agent()")]
    pub user_agent: String,

    /// URL of a `pnpr` server. When set, `pacquet install` offloads
    /// dependency resolution and file fetching to the server: it sends
    /// its own registry configuration, the server resolves against those
    /// registries and streams back the files the local store is missing,
    /// and `node_modules` is then linked locally from the
    /// server-produced lockfile (like server-side rendering — the
    /// compute runs remotely, the result is materialized locally).
    /// `None` runs the normal local resolution flow.
    pub pnpr_server: Option<String>,

    /// Cargo dependency management declared by the workspace.
    pub cargo: CargoSettings,
    pub python: PythonSettings,

    pub remote_side_effects_cache: Option<RemoteSideEffectsCacheSettings>,

    /// `sideEffectsCache.read` and `.write` as declared, which
    /// [`Config::side_effects_cache_read`] and
    /// [`Config::side_effects_cache_write`] prefer over the boolean pair.
    ///
    /// The pair cannot express every combination the declaration can: reading
    /// without writing is `sideEffectsCacheReadonly`, but writing without
    /// reading — populate a cache this run never consumes, which is what a
    /// warming CI job wants — has no spelling in it at all.
    pub side_effects_cache_read_setting: Option<bool>,
    pub side_effects_cache_write_setting: Option<bool>,

    /// Path to the user-level `.npmrc` to read auth from, overriding the
    /// default `~/.npmrc`. The `npmrcAuthFile` setting (and the
    /// `--userconfig` alias). Resolved in [`Config::current`] from this
    /// field (set by the CLI flag) then the `PNPM_CONFIG_NPMRC_AUTH_FILE`
    /// / `PNPM_CONFIG_USERCONFIG` / `npm_config_userconfig` env vars.
    /// `None` falls back to `~/.npmrc`.
    pub npmrc_auth_file: Option<PathBuf>,

    /// Directory containing the nearest ancestor `pnpm-workspace.yaml`.
    /// Set by [`WorkspaceSettings::apply_to`](crate::WorkspaceSettings::apply_to) when yaml was found, so
    /// later install-time code (notably [`resolve_and_group`] for
    /// `patchedDependencies`) can resolve relative paths against the
    /// same dir pnpm does. `None` when no `pnpm-workspace.yaml` exists
    /// anywhere up the tree — in that case there are no patches /
    /// allowBuilds settings to resolve either.
    pub workspace_dir: Option<PathBuf>,

    /// Raw `patchedDependencies` from `pnpm-workspace.yaml`: keys are
    /// `name[@version]`, values are patch file paths (relative to
    /// `workspace_dir` or absolute). Consumed by
    /// [`Config::resolved_patched_dependencies`] which performs the
    /// path resolution and SHA-256 hashing.
    ///
    /// [`IndexMap`] preserves user-specified order so range entries
    /// land in `PatchGroup.range` in the same order they appear in
    /// yaml — keeping `PATCH_KEY_CONFLICT` diagnostics aligned.
    ///
    /// pnpm v11 reads `patchedDependencies` from `pnpm-workspace.yaml`
    /// only.
    pub patched_dependencies: Option<IndexMap<String, String>>,

    /// Precomputed `patchedDependencies` hashes supplied by a remote
    /// resolver. Resolution only needs the hashes to key patched package
    /// snapshots; the client retains the file paths and applies the patches
    /// while materializing the returned lockfile.
    pub patched_dependency_hashes_override: Option<IndexMap<String, String>>,

    /// Raw `patchesDir` setting used by `patch-commit` when writing
    /// generated patch files. `None` means the command default
    /// (`patches`) applies.
    pub patches_dir: Option<String>,

    /// Explicit pnpmfiles resolved against the workspace root. `None`
    /// discovers the default `.pnpmfile.mjs` or `.pnpmfile.cjs`.
    pub pnpmfile: Option<Vec<PathBuf>>,

    /// `globalPnpmfile`. Loaded ahead of every project pnpmfile and left out
    /// of `pnpmfileChecksum`, matching the entry pnpm's `requireHooks` pushes
    /// first with `includeInChecksum: false`. A user-level file the lockfile
    /// therefore cannot vouch for.
    pub global_pnpmfile: Option<PathBuf>,

    /// `allowUnusedPatches` from `pnpm-workspace.yaml`. When `true`,
    /// configured patches that don't match any installed dependency
    /// produce a warning instead of failing the install with
    /// `ERR_PNPM_UNUSED_PATCH`. Default `false` — unused patches are
    /// an error.
    pub allow_unused_patches: bool,

    /// Raw `configDependencies` from `pnpm-workspace.yaml`: package
    /// name → version-with-integrity spec. Recorded verbatim in the
    /// workspace-state file so pnpm's `checkDepsStatus` sees the same
    /// value it holds in the live config and doesn't treat the install
    /// as stale. See [`WorkspaceSettings::config_dependencies`].
    ///
    /// [`WorkspaceSettings::config_dependencies`]: crate::workspace_yaml::WorkspaceSettings::config_dependencies
    pub config_dependencies: Option<BTreeMap<String, ConfigDependency>>,

    /// `pnpm.allowBuilds` from `pnpm-workspace.yaml`: package names
    /// (or `name@version` keys) that are allowed to run lifecycle
    /// scripts. pnpm 11 denies scripts by default; the allow-list is
    /// the opt-in mechanism. Consumed by `AllowBuildPolicy::from_config`
    /// in `pnpm-package-manager`.
    ///
    /// Default empty.
    pub allow_builds: HashMap<String, bool>,

    /// `dangerouslyAllowAllBuilds` from `pnpm-workspace.yaml`. When
    /// `true`, every package may run lifecycle scripts regardless of
    /// `allow_builds`. Default `false` to match pnpm v11.
    pub dangerously_allow_all_builds: bool,

    /// `strictDepBuilds` from `pnpm-workspace.yaml`. When `true` (the
    /// default), an install that ignores any dependency build script
    /// fails with `ERR_PNPM_IGNORED_BUILDS` instead of only warning.
    #[default(true)]
    pub strict_dep_builds: bool,

    /// `ignoreScripts` (`--ignore-scripts`). When `true`, no lifecycle
    /// scripts run — neither dependency build scripts
    /// (`preinstall`/`install`/`postinstall`) nor the project's own
    /// lifecycle scripts. Dependency builds that would otherwise be
    /// reported as ignored are not collected, so the install does not
    /// fail with `ERR_PNPM_IGNORED_BUILDS` under `strictDepBuilds`.
    /// The during-install build loop skips its allow-build gate entirely
    /// when set, leaving `ignoredBuilds` empty. Default `false`.
    pub ignore_scripts: bool,

    /// `ignorePnpmfile` (`--ignore-pnpmfile`). When `true`, no pnpmfile hooks
    /// run: neither the pnpmfiles the project configures or ships nor those of
    /// config-dependency plugins are loaded, so `readPackage`, `updateConfig`,
    /// `afterAllResolved`, custom resolvers and custom fetchers are all
    /// skipped. Settable from configuration and the environment as well as the
    /// flag, which ORs on top — pnpm carries `ignore-pnpmfile` in both its
    /// config-file keys and its schema. Default `false`.
    pub ignore_pnpmfile: bool,

    /// `gitChecks` (`--no-git-checks`). When `true` (the default),
    /// `pnpm publish` verifies the git working tree is clean, on the
    /// expected branch, and up to date with the remote before publishing.
    /// Setting it to `false` — via `git-checks=false` in `.npmrc`,
    /// `gitChecks: false` in `pnpm-workspace.yaml`, or the `--no-git-checks`
    /// flag — skips those checks. Mirrors pnpm's `opts.gitChecks !== false` gate.
    #[default(true)]
    pub git_checks: bool,

    /// `scriptsPrependNodePath` from `pnpm-workspace.yaml`. Controls
    /// whether `dirname(node_execpath)` is prepended to `PATH` when
    /// running lifecycle scripts. Default `Never` (`scriptsPrependNodePath:
    /// false`). Yaml accepts `true` / `false` / `"warn-only"`.
    pub scripts_prepend_node_path: ScriptsPrependNodePath,

    /// `enablePrePostScripts` from `pnpm-workspace.yaml`. When `true`,
    /// `pnpm run <name>` also runs the `pre<name>` and `post<name>`
    /// scripts if they exist. Defaults to `true`.
    #[default = true]
    pub enable_pre_post_scripts: bool,

    /// `scriptShell` from `pnpm-workspace.yaml`. The shell used to run
    /// scripts and `pnpm exec`. `None` selects the platform default
    /// (`sh` on POSIX, `cmd.exe` on Windows).
    pub script_shell: Option<String>,

    /// `nodeOptions` from `pnpm-workspace.yaml`. When set, it is exported
    /// as `NODE_OPTIONS` to scripts and `pnpm exec` child processes.
    pub node_options: Option<String>,

    /// `extraBinPaths`: directories prepended to `PATH` (after the
    /// project's own `node_modules/.bin`) when running scripts and
    /// `pnpm exec`. Computed as the workspace root's
    /// `node_modules/.bin` inside a workspace and left empty
    /// otherwise, so workspace-root dev tools are callable from every
    /// member's scripts.
    pub extra_bin_paths: Vec<PathBuf>,

    /// `extraEnv`: extra environment variables exported to the lifecycle
    /// scripts and spawned child processes of a command. Empty by
    /// default. Not a `pnpm-workspace.yaml` key — the only way to
    /// populate it is an `updateConfig` pnpmfile hook that returns an
    /// `extraEnv` object, wired up in `pnpm_cli`'s
    /// `run_update_config_hooks`. That hook runs for the install family
    /// and commands that pack packages, making the returned environment
    /// available to their lifecycle scripts.
    pub extra_env: HashMap<String, String>,

    /// `unsafePerm` from `pnpm-workspace.yaml`. When `false`,
    /// lifecycle scripts run under a TMPDIR isolated to
    /// `node_modules/.tmp` and uid/gid drops to a non-root user.
    /// Pacquet honors the TMPDIR side (see
    /// `pnpm_executor::make_env`); the uid/gid drop is a no-op in
    /// practice because the npm-lifecycle fork never populates
    /// `opts.user` / `opts.group`, so it just re-applies the current
    /// process's uid/gid.
    ///
    /// The default is auto-detected via [`default_unsafe_perm`]:
    /// `true` on Windows or POSIX-not-root; `false` when running
    /// as root on POSIX. On Windows,
    /// [`WorkspaceSettings::apply_to`](crate::WorkspaceSettings::apply_to) also force-overrides the
    /// applied value to `true` regardless of yaml — a
    /// `process.platform === 'win32'` gate.
    #[default(_code = "default_unsafe_perm()")]
    pub unsafe_perm: bool,

    /// `childConcurrency` from `pnpm-workspace.yaml` — the maximum
    /// number of lifecycle-script spawns that may run in parallel
    /// inside a single `BuildModules` chunk. Resolved through
    /// [`resolve_child_concurrency`](crate::defaults::resolve_child_concurrency) so the yaml value can be
    /// negative (interpreted as `parallelism - |value|`).
    ///
    /// Default: `min(4, availableParallelism())`.
    /// Chunks run sequentially (children before parents); only
    /// members within a chunk are parallelized.
    #[default(_code = "default_child_concurrency()")]
    pub child_concurrency: u32,

    /// `workspaceConcurrency` from `pnpm-workspace.yaml` / global
    /// `config.yaml` / `PNPM_CONFIG_WORKSPACE_CONCURRENCY`, overridable
    /// per-invocation by the `--workspace-concurrency` CLI flag. The
    /// maximum number of workspace projects pnpm processes in parallel
    /// during a recursive operation. Resolved through
    /// [`resolve_child_concurrency`](crate::defaults::resolve_child_concurrency) so a non-positive yaml/CLI value is
    /// read as `parallelism - |value|` (floored at 1).
    ///
    /// Default: `min(4, availableParallelism())`.
    ///
    /// Parsed and stored for parity with pnpm's config surface.
    /// pacquet's frozen-lockfile install materializes the whole
    /// workspace in a single shared pass rather than one project at a
    /// time, so there is no per-project parallel loop for this limit
    /// to throttle yet — the same "read now, consume as the
    /// architecture lands" posture as [`Self::prefer_offline`].
    #[default(_code = "default_workspace_concurrency()")]
    pub workspace_concurrency: u32,

    /// `--recursive` / `-r`. When set, a command operates on every
    /// project in the workspace rather than only the project in the
    /// current directory. A CLI-only boolean: it is not a `.npmrc` /
    /// `pnpm-workspace.yaml` key, so the yaml / env overlay never
    /// populates it — the CLI layer sets it from the flag.
    ///
    /// pacquet's install already spans the whole workspace (it reads
    /// every importer from the shared lockfile), so the flag is a
    /// surface no-op on `install` today. Stored for parity and for
    /// future commands where recursive vs. single-project selection
    /// diverges.
    pub recursive: bool,

    /// `--filter` selectors, one raw selector string per entry
    /// (`@scope/*`, `./pkg`, `foo...`, `!bar`, ...), parsed by
    /// `pnpm-workspace-projects-filter`. A CLI-only array: not a
    /// `.npmrc` / `pnpm-workspace.yaml` key, so only the CLI layer
    /// populates it.
    pub filter: Vec<String>,

    /// `--filter-prod` selectors. Same shape as [`Self::filter`], but
    /// each selector follows production dependencies only when its
    /// dependency walk runs. A CLI-only array.
    pub filter_prod: Vec<String>,

    /// `--workspace-root` / `-w`: run the command on the root workspace
    /// project. CLI-only, like [`Self::filter`].
    pub workspace_root: bool,

    /// `--fail-if-no-match`: exit with code 1 when the `--filter` /
    /// `--filter-prod` selectors select no workspace project, instead of
    /// letting the command run over an empty selection. CLI-only, like
    /// [`Self::filter`].
    pub fail_if_no_match: bool,

    /// `includeWorkspaceRoot` — whether a recursive command also runs on
    /// the workspace root project. `run`, `exec`, `add`, and `test`
    /// exclude the root from an unnarrowed recursive selection; this
    /// setting keeps it in. Universal `--include-workspace-root` /
    /// `--no-include-workspace-root` flag, `pnpm-workspace.yaml` key, and
    /// `PNPM_CONFIG_INCLUDE_WORKSPACE_ROOT`.
    pub include_workspace_root: bool,

    /// `ignoreWorkspaceCycles` — suppress the report a recursive install
    /// makes when the selected workspace projects depend on each other
    /// in a cycle. See [`Self::disallow_workspace_cycles`] for what the
    /// report is.
    pub ignore_workspace_cycles: bool,

    /// `disallowWorkspaceCycles` — make a cycle among the selected
    /// workspace projects an error (`ERR_PNPM_DISALLOW_WORKSPACE_CYCLES`)
    /// rather than a warning. [`Self::ignore_workspace_cycles`] wins over
    /// it: nothing is reported at all under that setting.
    pub disallow_workspace_cycles: bool,

    /// `testPattern` from `pnpm-workspace.yaml` /
    /// `PNPM_CONFIG_TEST_PATTERN`, overridable by the `--test-pattern`
    /// CLI flag. Glob patterns naming test files: when a `[<since>]`
    /// changed-packages filter selects a project whose changed files
    /// all match, the project is selected without its dependents.
    pub test_pattern: Vec<String>,

    /// `legacyDirFiltering` — match a `{<dir>}` filter selector by
    /// directory subtree instead of by glob. Glob matching, the default,
    /// selects the project whose own directory matches the pattern; the
    /// legacy subtree matching selects the projects strictly below that
    /// directory instead.
    pub legacy_dir_filtering: bool,

    /// `syncInjectedDepsAfterScripts` from `pnpm-workspace.yaml` /
    /// `PNPM_CONFIG_SYNC_INJECTED_DEPS_AFTER_SCRIPTS`. Names the scripts
    /// after which every injected copy of the package that ran them is
    /// re-synced from its source.
    pub sync_injected_deps_after_scripts: Vec<String>,

    /// `changedFilesIgnorePattern` from `pnpm-workspace.yaml` /
    /// `PNPM_CONFIG_CHANGED_FILES_IGNORE_PATTERN`, overridable by the
    /// `--changed-files-ignore-pattern` CLI flag. Glob patterns of
    /// changed files a `[<since>]` changed-packages filter ignores
    /// when mapping the git diff to changed projects.
    pub changed_files_ignore_pattern: Vec<String>,

    /// Git host names where pacquet should clone via `git init` +
    /// `git remote add` + `git fetch --depth 1 origin <commit>` instead
    /// of a full `git clone`. Saves bandwidth and disk when the remote
    /// only needs the pinned commit. The `gitShallowHosts` setting.
    ///
    /// The default list follows
    /// <https://github.com/npm/git/blob/1e1dbd26bd/lib/clone.js#L13-L19>.
    #[default(_code = "default_git_shallow_hosts()")]
    pub git_shallow_hosts: Vec<String>,

    /// `supportedArchitectures` from `pnpm-workspace.yaml`. Threaded
    /// into the installability check at install time (via
    /// `pnpm-package-manager`'s `InstallabilityHost`, downstream of
    /// this crate) so optional platform-tagged dependencies for the
    /// listed `os` / `cpu` / `libc` values are kept even when they
    /// don't match the host triple. Per-axis CLI flags (`--cpu`,
    /// `--libc`, `--os`) override individual axes.
    /// Default `None` so the host triple is the sole accept set
    /// when neither yaml nor CLI sets a value.
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,

    /// `ignoredOptionalDependencies` from `pnpm-workspace.yaml`. A
    /// list of dep-name patterns the user wants entirely excluded
    /// from resolution + install. At manifest read time each
    /// matching key is dropped from `optionalDependencies` AND from
    /// `dependencies` (a package may list the same dep under both
    /// to make it optional only for some installers).
    ///
    /// The resolved set is also recorded on the lockfile so a
    /// subsequent install can detect drift between
    /// `pnpm-workspace.yaml` and the lockfile-recorded set —
    /// mismatch triggers `OutdatedLockfile`.
    pub ignored_optional_dependencies: Option<Vec<String>>,

    /// `overrides` from `pnpm-workspace.yaml`. Raw `selector → spec`
    /// map; see [`WorkspaceSettings::overrides`] for the field's
    /// contract. `$dep-name` self-references are resolved against
    /// the root manifest's direct deps before this field lands here.
    /// Empty maps collapse to `None`. Drives the read-package hook
    /// that rewrites manifests during install, and the lockfile-side
    /// drift check.
    ///
    /// [`WorkspaceSettings::overrides`]: crate::workspace_yaml::WorkspaceSettings::overrides
    pub overrides: Option<IndexMap<String, String>>,

    /// `packageExtensions` from `pnpm-workspace.yaml`. Maps a
    /// `name[@range]` selector to a partial manifest fragment that
    /// gets merged into every matching package's manifest at
    /// resolution time. The package's own fields win on conflict
    /// (`{ ...extension[field], ...manifest[field] }`), so an
    /// extension can only *add* missing entries — it never overrides
    /// a value the package already declares.
    ///
    /// Empty maps collapse to `None` (matches the `overrides` shape).
    /// See [`WorkspaceSettings::package_extensions`] for the yaml
    /// contract and
    /// [`PackageExtension`](crate::PackageExtension) for the entry shape.
    ///
    /// [`WorkspaceSettings::package_extensions`]: crate::workspace_yaml::WorkspaceSettings::package_extensions
    pub package_extensions: Option<IndexMap<String, workspace_yaml::PackageExtension>>,

    /// `packageConfigs` from `pnpm-workspace.yaml`, flattened to the
    /// `project name → settings` lookup
    /// [`Self::anchor_dedicated_project`] reads. Empty maps collapse
    /// to `None`. See [`ProjectConfig`] for the settings an entry may
    /// carry.
    pub package_configs: Option<IndexMap<String, ProjectConfig>>,

    /// pnpm's packument cache directory. Used by the lockfile
    /// verification gate to memoize past results in
    /// `<cache_dir>/lockfile-verified.jsonl`, and by the npm verifier
    /// to mirror full-metadata responses for conditional GETs.
    /// Share a writable cache only between mutually trusted users,
    /// jobs, and processes.
    ///
    /// The `cacheDir` setting.
    #[default(_code = "default_cache_dir::<Host>()")]
    pub cache_dir: PathBuf,

    /// `dlxCacheMaxAge`: the maximum age in **minutes** of a cached
    /// `pnpm dlx` install before it is rebuilt from scratch. Defaults to
    /// `1440` (24 hours).
    #[default(_code = "24 * 60")]
    pub dlx_cache_max_age: u64,

    /// Minimum age, in **minutes**, a published version must reach
    /// before pacquet accepts it. Drives the
    /// `MINIMUM_RELEASE_AGE_VIOLATION` verifier check on every
    /// `(name, version)` entry the lockfile loads under this policy.
    /// `None` disables the check entirely.
    ///
    /// Default: `Some(1440)` (24 hours). The `minimumReleaseAge`
    /// setting in minutes — the same unit pnpm's CLI / yaml accept and
    /// pnpm forwards verbatim to the verifier.
    #[default(_code = "Some(24 * 60)")]
    pub minimum_release_age: Option<u64>,

    /// Glob-style `name[@version]` patterns that opt specific packages
    /// out of the [`minimum_release_age`] check. Empty / `None` means
    /// no exclusions. The `minimumReleaseAgeExclude` setting.
    ///
    /// [`minimum_release_age`]: Self::minimum_release_age
    pub minimum_release_age_exclude: Option<Vec<String>>,

    /// When `true`, the resolving commands (`install`, `dedupe`, `add`,
    /// `remove`, `update`) prune [`Self::minimum_release_age_exclude`]
    /// entries in `pnpm-workspace.yaml` whose versions the freshly
    /// resolved lockfile no longer records, once the install has written
    /// that lockfile. The `minimumReleaseAgeExcludePrune` setting;
    /// default `false`, matching pnpm.
    pub minimum_release_age_exclude_prune: bool,

    /// When the registry's metadata lacks the per-version `time`
    /// field (some self-hosted registries strip it), the verifier
    /// cannot enforce the maturity cutoff. With this flag set,
    /// uncheckable entries pass with a one-time `globalWarn` instead
    /// of failing closed. The `minimumReleaseAgeIgnoreMissingTime`
    /// setting defaults to `true` so a registry that strips `time`
    /// (a self-hosted Verdaccio without provenance plugin, for
    /// example) doesn't lock the user out.
    #[default = true]
    pub minimum_release_age_ignore_missing_time: bool,

    /// When `true`, picks fresher-than-cutoff versions still abort
    /// rather than auto-collect into [`Self::minimum_release_age_exclude`].
    /// Used by the resolver path; the verifier itself does not gate
    /// on this flag. The `minimumReleaseAgeStrict` setting.
    ///
    /// Conditional default: `true` when `minimumReleaseAge` is
    /// explicitly configured, `false` otherwise. Modeled as [`Option`]
    /// here so the deserializer can
    /// distinguish "unset" from "explicit `false`"; the install path
    /// resolves the effective value via
    /// [`Self::resolved_minimum_release_age_strict`].
    pub minimum_release_age_strict: Option<bool>,

    /// Skip the lockfile supply-chain verification pass entirely. When
    /// `true`, the install trusts the lockfile as-is and never calls
    /// `verify_lockfile_resolutions`, even if other policies
    /// (`minimum_release_age`, `trust_policy`) are active. Use only in
    /// environments where the lockfile is effectively part of the
    /// trusted base — closed-source projects with trusted committers,
    /// fully reproducible CI against an already-verified lockfile. A
    /// poisoned lockfile (e.g. one a contributor authored under a
    /// weaker policy than CI enforces) will slip through. The
    /// `trustLockfile` setting.
    ///
    /// Added for [#11860](https://github.com/pnpm/pnpm/issues/11860):
    /// on multi-thousand-entry workspaces, the verification pass holds
    /// the per-package registry metadata needed for the trust check
    /// resident in memory and can OOM CI runners with a 2GB heap cap.
    /// Default `false` — verification stays on by default.
    pub trust_lockfile: bool,

    /// Trust-evidence policy applied to lockfile entries; see
    /// [`TrustPolicy`].
    pub trust_policy: TrustPolicy,

    /// `init-package-manager` / `initPackageManager` config: whether
    /// `pnpm init` pins a pnpm version in the manifest it scaffolds,
    /// through both `devEngines.packageManager` and the legacy
    /// `packageManager` field. Only the workspace root is pinned — a
    /// member of an existing workspace inherits the root's pin. The version
    /// pinned is the registry's `latest`, resolved by `pnpm-cli`'s
    /// `cli_args::init::version_to_pin`, which falls back to the running
    /// version whenever `latest` is unavailable, unusable, or older — see
    /// there for the cases.
    ///
    /// Defaults to `true`.
    #[default = true]
    pub init_package_manager: bool,

    /// `init-type` / `initType` config: the module system `pnpm init`
    /// records for the package it scaffolds. See [`InitType`].
    ///
    /// Defaults to `module`.
    pub init_type: InitType,

    /// `init-author-name` / `initAuthorName` config: the name part of the
    /// `name <email> (url)` author `pnpm init` writes.
    pub init_author_name: Option<String>,

    /// `init-author-email` / `initAuthorEmail` config: the email part of
    /// the author `pnpm init` writes. See [`Self::init_author_name`].
    pub init_author_email: Option<String>,

    /// `init-author-url` / `initAuthorUrl` config: the url part of the
    /// author `pnpm init` writes. See [`Self::init_author_name`].
    pub init_author_url: Option<String>,

    /// `init-license` / `initLicense` config: the `license` field
    /// `pnpm init` writes, replacing the `ISC` the scaffold carries.
    pub init_license: Option<String>,

    /// `init-version` / `initVersion` config: the `version` field
    /// `pnpm init` writes, replacing the `1.0.0` the scaffold carries.
    pub init_version: Option<String>,

    /// `pm-on-fail` / `pmOnFail` config: what to do when the project's
    /// `packageManager` / `devEngines.packageManager` pin doesn't match the
    /// running pnpm. See [`PmOnFail`]. Stays optional so the
    /// package-manager check applies the documented `download` default
    /// when unset.
    pub pm_on_fail: Option<PmOnFail>,

    /// `verify-deps-before-run` / `verifyDepsBeforeRun` config: what
    /// `pnpm run` / `pnpm exec` do when `node_modules` is out of sync
    /// with the lockfile. See [`VerifyDepsBeforeRun`]. Default
    /// `'install'` (`'verify-deps-before-run': 'install'`).
    #[default(VerifyDepsBeforeRun::Install)]
    pub verify_deps_before_run: VerifyDepsBeforeRun,

    /// `audit-level` / `auditLevel` config for `pnpm audit`.
    pub audit_level: Option<AuditLevel>,

    /// `auditConfig` config for `pnpm audit`.
    pub audit_config: AuditConfig,

    /// `audit.ignorePrune` from `pnpm-workspace.yaml`. See
    /// [`AuditSettings::ignore_prune`](crate::AuditSettings::ignore_prune).
    pub audit_ignore_prune: Option<bool>,

    /// `versioning` from `pnpm-workspace.yaml`: native workspace release
    /// management, consumed by `pnpm change` and the bare `pnpm version -r`.
    pub versioning: pnpm_versioning::VersioningSettings,

    /// Glob-style `name[@version]` patterns that opt specific packages
    /// out of the [`trust_policy`] check. The `trustPolicyExclude`
    /// setting.
    ///
    /// [`trust_policy`]: Self::trust_policy
    pub trust_policy_exclude: Option<Vec<String>>,

    /// When `true`, the resolving commands (`install`, `dedupe`, `add`,
    /// `remove`, `update`) prune [`Self::trust_policy_exclude`] entries
    /// in `pnpm-workspace.yaml` whose versions the freshly resolved
    /// lockfile no longer records, once the install has written that
    /// lockfile. The `trustPolicyExcludePrune` setting; default
    /// `false`, matching pnpm.
    pub trust_policy_exclude_prune: bool,

    /// Cutoff in minutes after which the trust check skips a
    /// version that's old enough — once a package has been published
    /// for long enough, the supply-chain assumption is that any
    /// downgrade would have already surfaced. The `trustPolicyIgnoreAfter`
    /// setting.
    pub trust_policy_ignore_after: Option<u64>,

    /// How direct dependencies pick a version when several satisfy the
    /// wanted range, and whether subdependencies are constrained by
    /// publication date. See [`ResolutionMode`]. Default
    /// [`ResolutionMode::Highest`] (`'resolution-mode': 'highest'`).
    pub resolution_mode: ResolutionMode,

    /// How `pnpm add` / `pnpm update` reconcile a directly-specified
    /// version against a matching `catalog:` entry. See [`CatalogMode`].
    /// Default [`CatalogMode::Manual`] (`'catalog-mode': 'manual'`).
    pub catalog_mode: CatalogMode,

    /// When `true`, commands that persist the workspace manifest
    /// (`add`, `remove`, `update`) also drop entries of the `catalog:`
    /// and `catalogs:` blocks that no workspace project references. The
    /// `catalogPrune` setting (formerly `cleanupUnusedCatalogs`, still
    /// accepted); default `false`, matching pnpm.
    pub catalog_prune: bool,

    /// Catalogs injected by an `updateConfig` pnpmfile hook, seeded from
    /// `pnpm-workspace.yaml`'s `catalog:`/`catalogs:` and returned
    /// (possibly modified) by the hook. `None` when no hook changed
    /// them, in which case consumers read catalogs straight from the
    /// workspace manifest. `Some` carries the complete catalog set the
    /// hook produced (existing + injected), so consumers use it as-is
    /// — the counterpart to pnpm's `config.catalogs` after the
    /// `updateConfig` pass.
    pub catalogs: Option<pnpm_catalogs_types::Catalogs>,

    /// Name of the catalog `pnpm add` saves a new dependency into,
    /// set by `--save-catalog-name=<name>` (with `--save-catalog` a
    /// shorthand for `default`). When `Some`, an `add` writes
    /// `catalog:`/`catalog:<name>` to the manifest and inserts the
    /// entry into `pnpm-workspace.yaml` even under
    /// [`CatalogMode::Manual`]. The `saveCatalogName` setting (default
    /// `undefined`).
    pub save_catalog_name: Option<String>,

    /// The range operator `pnpm add` prepends to a resolved version
    /// when saving it: `^` (the default), `~`, or `""` for an exact
    /// pin. The `savePrefix` setting, overridden per-invocation by
    /// `--save-prefix` / `--save-exact`.
    pub save_prefix: Option<String>,

    /// Whether `pnpm add` saves the resolved version exactly, with no
    /// range operator. The `saveExact` setting, equivalent to passing
    /// `--save-exact`.
    pub save_exact: bool,

    /// Whether `pnpm add` also records the new dependency in
    /// `peerDependencies` (and saves it as a dev dependency). The
    /// `savePeer` setting, equivalent to passing `--save-peer`.
    pub save_peer: bool,

    /// Whether the configured registry returns the per-version `time`
    /// field in its *abbreviated* metadata. When `false` (the default),
    /// [`ResolutionMode::TimeBased`] resolution (and the
    /// [`TrustPolicy::NoDowngrade`] check) must fetch full metadata to
    /// obtain publication dates. Setting this to `true` for a registry
    /// that includes `time` in abbreviated metadata (Verdaccio 5.15.1+)
    /// avoids the slower full-metadata fetch. The
    /// `registrySupportsTimeField` setting (default `false`).
    pub registry_supports_time_field: bool,

    /// `name → semver-range` map of deprecated package versions whose
    /// deprecation warning should be suppressed. A deprecated package
    /// is reported unless its name has an entry here whose range the
    /// resolved version satisfies. The `allowedDeprecatedVersions`
    /// setting.
    ///
    /// Parsed and stored for parity with pnpm's config surface. Pacquet
    /// does not yet emit deprecation warnings during resolution, so
    /// there is nothing for the allow-list to suppress today; the field
    /// is consumed once that warning path lands.
    pub allowed_deprecated_versions: BTreeMap<String, String>,

    /// `updateConfig` from `pnpm-workspace.yaml`: defaults specific to
    /// `pnpm update`, including changeset generation, dependency-name
    /// patterns the command skips, and whether GitHub Actions should be
    /// updated.
    pub update_config: workspace_yaml::UpdateConfig,

    /// `tasks` from `pnpm-workspace.yaml`: the workspace's task
    /// declarations, consumed by the recursive `run` task scheduler. See
    /// [`workspace_yaml::TaskSettings`]. Empty when the workspace declares
    /// none.
    pub tasks: IndexMap<String, workspace_yaml::TaskSettings>,

    /// `pipelines` from `pnpm-workspace.yaml`: named sets of task requests
    /// for `pnpm pipeline`, keyed by pipeline name. Empty when the
    /// workspace declares none.
    pub pipelines: IndexMap<String, Vec<String>>,

    /// `pipelineBase` from `pnpm-workspace.yaml`: the git ref
    /// `pnpm pipeline` resolves its affected-selection merge base against.
    /// `None` falls back to the command's default.
    pub pipeline_base: Option<String>,

    /// `peerDependencyRules` from `pnpm-workspace.yaml`: customizations
    /// applied when reporting peer-dependency issues. See
    /// [`PeerDependencyRules`].
    ///
    /// Parsed and stored for parity with pnpm's config surface. Pacquet
    /// resolves peers but does not yet have a missing/bad peer-issue
    /// reporting pass, so these rules have no consumer today; they are
    /// applied once that pass lands.
    ///
    /// [`PeerDependencyRules`]: crate::workspace_yaml::PeerDependencyRules
    pub peer_dependency_rules: workspace_yaml::PeerDependencyRules,

    /// Per-registry `Authorization` header lookup, populated from
    /// `.npmrc` auth keys (`_auth`, `_authToken`, `username`/`_password`,
    /// scoped variants). Threaded through the network and tarball
    /// fetchers via [`pnpm_network::AuthHeaders::for_url`]. Empty
    /// when no `.npmrc` was found or no auth keys were set.
    pub auth_headers: std::sync::Arc<pnpm_network::AuthHeaders>,

    /// Raw `_authToken` values keyed by the nerf-darted registry URI
    /// (`//host[:port]/path/`), for the default (registry-wide) scope.
    /// Unlike [`Self::auth_headers`], which bakes credentials into
    /// ready-to-send `Authorization` header values and discards the
    /// raw token, this preserves the unmodified token so commands like
    /// `pnpm logout` can read it back to revoke it on the registry.
    /// The subset of raw auth config the auth commands consult.
    pub auth_tokens_by_uri: std::collections::HashMap<String, String>,

    /// Every registry credential, keyed `[uri][scope]` the way pnpm's
    /// `configByUri` is: the nerf-darted registry URI, then the package
    /// scope it is for, with [`pnpm_network::DEFAULT_REGISTRY_SCOPE`] for
    /// the registry-wide credential. This is what an `updateConfig` hook
    /// reads; the fetchers read [`Self::auth_headers`].
    pub registry_creds_by_uri:
        std::collections::HashMap<String, BTreeMap<String, npmrc_auth::RegistryCreds>>,

    pub package_manager_bootstrap: PackageManagerBootstrap,

    /// Camel-cased record of the settings the user *explicitly* set through
    /// `pnpm-workspace.yaml`, the global `config.yaml`, and `PNPM_CONFIG_*`
    /// env vars (with `_auth` excluded and `null` values dropped). Populated
    /// by [`Config::current`]; empty when a `Config` is built without it.
    ///
    /// This tracks the explicitly-set keys plus the merged config record
    /// consumed by `pnpm config get` / `pnpm config list`:
    /// because [`WorkspaceSettings`](crate::WorkspaceSettings)'s fields are `Option`s, a serialized
    /// settings struct names exactly the keys a source set, with the user's
    /// raw value. The `config` command turns this into the record it prints.
    pub explicit_settings: serde_json::Map<String, serde_json::Value>,

    /// Raw `.npmrc` / `auth.ini` config keys (those for which
    /// [`config_types::is_ini_config_key`](crate::config_types::is_ini_config_key) holds: `registry`, `@scope:registry`,
    /// `//host/:_authToken`, `username`, `ca`, ...), post-`${VAR}` substitution
    /// and merged across sources. The raw auth-config map, consumed by
    /// `pnpm config get` / `pnpm config list`.
    pub raw_auth_config: BTreeMap<String, String>,

    /// The global pnpm config directory (`<configDir>`), where `config.yaml`
    /// and `auth.ini` live. `None` when it cannot be determined. Consumed by
    /// `pnpm config` and by `globalconfig` lookups.
    pub config_dir: Option<PathBuf>,
}

impl Config {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolved_patched_dependencies(
        &self,
    ) -> Result<Option<PatchGroupRecord>, ResolvePatchedDependenciesError> {
        if let Some(hashes) = self.patched_dependency_hashes_override.as_ref() {
            let groups = group_patched_dependencies(hashes.iter().map(|(key, hash)| {
                (key.clone(), PatchInput { hash: hash.clone(), patch_file_path: None })
            }))?;
            return Ok((!groups.is_empty()).then_some(groups));
        }
        let (Some(workspace_dir), Some(raw)) = (&self.workspace_dir, &self.patched_dependencies)
        else {
            return Ok(None);
        };
        resolve_and_group(workspace_dir, raw)
    }

    /// Resolve relative patch file paths in
    /// [`Config::patched_dependencies`] against
    /// [`Config::workspace_dir`] and hash each file, producing the
    /// `patchedDependencies` map the lockfile records: each configured
    /// key mapped to its patch file's SHA-256 hex digest.
    ///
    /// Distinct from [`Self::resolved_patched_dependencies`], which
    /// groups the same entries by package name for the resolver — this
    /// keeps the user's verbatim keys so the lockfile is byte-faithful
    /// (e.g. a bare `foo` and `foo@*` stay separate keys rather than
    /// collapsing into one group bucket).
    ///
    /// Returns `Ok(None)` when either field is unset.
    pub fn patched_dependency_hashes(
        &self,
    ) -> Result<Option<BTreeMap<String, String>>, CalcPatchHashError> {
        Ok(self
            .patched_dependency_hashes_in_config_order()?
            .map(|hashes| hashes.into_iter().collect()))
    }

    /// Return patch hashes in configured selector order.
    ///
    /// Precomputed overrides avoid file reads. Without an override, each
    /// configured patch file is hashed and any I/O or hashing error is
    /// propagated. Returns `None` when no non-empty patch configuration is
    /// available.
    pub fn patched_dependency_hashes_in_config_order(
        &self,
    ) -> Result<Option<IndexMap<String, String>>, CalcPatchHashError> {
        if let Some(hashes) = self.patched_dependency_hashes_override.as_ref() {
            return Ok((!hashes.is_empty()).then(|| hashes.clone()));
        }
        let (Some(workspace_dir), Some(raw)) = (&self.workspace_dir, &self.patched_dependencies)
        else {
            return Ok(None);
        };
        let mut hashes = IndexMap::with_capacity(raw.len());
        for (key, rel_or_abs) in raw {
            let candidate = Path::new(rel_or_abs);
            let path = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                workspace_dir.join(candidate)
            };
            hashes.insert(key.clone(), create_hex_hash_from_file(&path)?);
        }
        Ok((!hashes.is_empty()).then_some(hashes))
    }

    /// Persist the config data until the program terminates.
    pub fn leak(self) -> &'static mut Self {
        self.pipe(Box::new).pipe(Box::leak)
    }
}

mod cache;
