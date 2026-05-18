mod api;
mod defaults;
mod env_replace;
pub mod matcher;
mod npmrc_auth;
mod workspace_yaml;

pub use crate::api::{EnvVar, Host};

use indexmap::IndexMap;
use pacquet_patching::{PatchGroupRecord, ResolvePatchedDependenciesError, resolve_and_group};
use pacquet_store_dir::StoreDir;
use pipe_trait::Pipe;
use serde::Deserialize;
use smart_default::SmartDefault;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    path::PathBuf,
};

pub use crate::defaults::{
    available_parallelism, default_git_shallow_hosts, default_unsafe_perm, is_unsafe_perm_posix,
    resolve_child_concurrency,
};
use crate::defaults::{
    default_child_concurrency, default_enable_global_virtual_store, default_fetch_retries,
    default_fetch_retry_factor, default_fetch_retry_maxtimeout, default_fetch_retry_mintimeout,
    default_hoist_pattern, default_modules_cache_max_age, default_modules_dir,
    default_public_hoist_pattern, default_registry, default_store_dir, default_virtual_store_dir,
};
pub use workspace_yaml::{
    LoadWorkspaceYamlError, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, workspace_root_or,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeLinker {
    /// dependencies are symlinked from a virtual store at node_modules/.pnpm.
    #[default]
    Isolated,

    /// flat node_modules without symlinks is created. Same as the node_modules created by npm or
    /// Yarn Classic.
    Hoisted,

    /// no node_modules. Plug'n'Play is an innovative strategy for Node that is used by
    /// Yarn Berry. It is recommended to also set symlink setting to false when using pnp as
    /// your linker.
    Pnp,
}

/// Tri-state mirror of `pacquet_executor::ScriptsPrependNodePath`
/// with serde wiring. The executor crate keeps its own enum free of
/// serde so config concerns don't leak into the spawn-path. Converted
/// at the `BuildModules` call site (see `install_frozen_lockfile.rs`)
/// via an explicit `match`; no `From` impl exists because neither
/// crate depends on the other, and adding such a dep just for the
/// conversion would invert the layering. Both enums share the same
/// three variants so the match is exhaustive and one-line per arm.
///
/// Deserializes the upstream `scriptsPrependNodePath: boolean | 'warn-only'`
/// yaml shape ([`Config.scriptsPrependNodePath`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/Config.ts#L108)).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScriptsPrependNodePath {
    /// `scriptsPrependNodePath: true` — always prepend.
    Always,
    /// `scriptsPrependNodePath: false` (or absent) — never prepend.
    #[default]
    Never,
    /// `scriptsPrependNodePath: 'warn-only'` — emit a warning if the
    /// node in PATH differs from the running interpreter, do not
    /// prepend.
    WarnOnly,
}

impl<'de> serde::Deserialize<'de> for ScriptsPrependNodePath {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ScriptsPrependNodePath;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a boolean or the string "warn-only""#)
            }
            fn visit_bool<DeError: de::Error>(self, value: bool) -> Result<Self::Value, DeError> {
                Ok(if value {
                    ScriptsPrependNodePath::Always
                } else {
                    ScriptsPrependNodePath::Never
                })
            }
            fn visit_str<DeError: de::Error>(self, value: &str) -> Result<Self::Value, DeError> {
                match value {
                    "warn-only" => Ok(ScriptsPrependNodePath::WarnOnly),
                    other => Err(DeError::invalid_value(
                        de::Unexpected::Str(other),
                        &r#"true, false, or "warn-only""#,
                    )),
                }
            }
        }
        deserializer.deserialize_any(V)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageImportMethod {
    ///  try to clone packages from the store. If cloning is not supported then hardlink packages
    /// from the store. If neither cloning nor linking is possible, fall back to copying
    #[default]
    Auto,

    /// hard link packages from the store
    Hardlink,

    /// copy packages from the store
    Copy,

    /// clone (AKA copy-on-write or reference link) packages from the store
    Clone,

    /// try to clone packages from the store. If cloning is not supported then fall back to copying
    CloneOrCopy,
}

/// Resolved runtime config built from defaults, the auth subset of
/// `.npmrc`, and `pnpm-workspace.yaml` (see [`Config::current`]).
///
/// The type carries the merged result — it is never deserialized from a
/// file directly. Yaml is parsed into [`WorkspaceSettings`] and applied
/// onto `Config` field-by-field, mirroring pnpm 11's split between
/// `.npmrc` (auth/registry/network) and `pnpm-workspace.yaml`
/// (project-structural settings).
#[derive(Debug, SmartDefault)]
pub struct Config {
    /// When true, all dependencies are hoisted to node_modules/.pnpm/node_modules.
    /// This makes unlisted dependencies accessible to all packages inside node_modules.
    #[default = true]
    pub hoist: bool,

    /// Tells pnpm which packages should be hoisted to node_modules/.pnpm/node_modules.
    /// By default, all packages are hoisted - however, if you know that only some flawed packages
    /// have phantom dependencies, you can use this option to exclusively hoist the phantom
    /// dependencies (recommended).
    ///
    /// `None` mirrors upstream's `null`: hoisting on the private side
    /// is disabled. `Some([])` is "feature on but no pattern matches",
    /// which still triggers the hoist pass (in case `public_hoist_pattern`
    /// is set). `Some(non-empty)` is the normal case. The default is
    /// `Some(["*"])`, matching pnpm.
    ///
    /// The hoist guard at the install call site is
    /// `hoist_pattern.is_some() || public_hoist_pattern.is_some()` —
    /// see upstream's
    /// [`opts.hoistPattern != null || opts.publicHoistPattern != null`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L471)
    /// gate.
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
    /// Default is `Some(["*eslint*", "*prettier*"])`.
    #[default(_code = "Some(default_public_hoist_pattern())")]
    pub public_hoist_pattern: Option<Vec<String>>,

    /// By default, pnpm creates a semistrict node_modules, meaning dependencies have access to
    /// undeclared dependencies but modules outside of node_modules do not. With this layout,
    /// most of the packages in the ecosystem work with no issues. However, if some tooling only
    /// works when the hoisted dependencies are in the root of node_modules, you can set this to
    /// true to hoist them for you.
    pub shamefully_hoist: bool,

    /// The location where all the packages are saved on the disk.
    #[default(_code = "default_store_dir::<Host, _, _, _>(home::home_dir, env::current_dir)")]
    pub store_dir: StoreDir,

    /// The directory in which dependencies will be installed (instead of node_modules).
    #[default(_code = "default_modules_dir()")]
    pub modules_dir: PathBuf,

    /// Defines what linker should be used for installing Node packages.
    pub node_linker: NodeLinker,

    /// When symlink is set to false, pnpm creates a virtual store directory without any symlinks.
    /// It is a useful setting together with node-linker=pnp.
    #[default = true]
    pub symlink: bool,

    /// The directory with links to the store. All direct and indirect dependencies of the
    /// project are linked into this directory.
    ///
    /// When [`enable_global_virtual_store`] is `true` and the user has not
    /// explicitly set this field, [`Config::current`] re-points it at
    /// `<store_dir>/v11/links` to mirror upstream's
    /// [`extendInstallOptions.ts:350-358`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-installer/src/install/extendInstallOptions.ts#L350-L358).
    /// The `v11/` segment comes from pnpm's [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42),
    /// which appends `STORE_VERSION` to the configured `storeDir`
    /// before `extendInstallOptions` runs its `path.join(storeDir,
    /// 'links')` — so the join lands one level deeper than the
    /// configured root.
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
    /// Default `false` — matches pnpm v11's effective default for
    /// non-`--global` installs. The `true` assignment at
    /// [`config/reader/src/index.ts:392-394`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L392-L394)
    /// applies only inside upstream's `if (cliOptions['global'])`
    /// block (see `default_enable_global_virtual_store` in
    /// `crates/config/src/defaults.rs` for the full reasoning).
    /// Pacquet has no `--global` flow, so the only applicable
    /// upstream default is `false`.
    #[default(_code = "default_enable_global_virtual_store()")]
    pub enable_global_virtual_store: bool,

    /// The shared global-virtual-store directory. When
    /// [`enable_global_virtual_store`] is `true` this is the same path as
    /// [`virtual_store_dir`]; when `false`, it is still computed as
    /// `<store_dir>/v11/links` (matching upstream's unconditional
    /// assignment at [`extendInstallOptions.ts:356-358`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-installer/src/install/extendInstallOptions.ts#L356-L358))
    /// even though no install path consults it in that mode today.
    ///
    /// Populated by [`Config::current`] after yaml has been applied; the
    /// `SmartDefault` value is overwritten there with the path derived
    /// from the resolved `store_dir` / `virtual_store_dir`. The default
    /// here is only meaningful when `Config::new()` is used in isolation
    /// (mostly tests).
    ///
    /// [`enable_global_virtual_store`]: Self::enable_global_virtual_store
    /// [`virtual_store_dir`]: Self::virtual_store_dir
    #[default(_code = "default_virtual_store_dir()")]
    pub global_virtual_store_dir: PathBuf,

    /// Controls the way packages are imported from the store (if you want to disable symlinks
    /// inside node_modules, then you need to change the node-linker setting, not this one).
    pub package_import_method: PackageImportMethod,

    /// The time in minutes after which orphan packages from the modules directory should be
    /// removed. pnpm keeps a cache of packages in the modules directory. This boosts installation
    /// speed when switching branches or downgrading dependencies.
    ///
    /// Default value is 10080 (7 days in minutes)
    #[default(_code = "default_modules_cache_max_age()")]
    pub modules_cache_max_age: u64,

    /// When set to false, pnpm won't read or generate a pnpm-lock.yaml file.
    pub lockfile: bool,

    /// When set to true and the available pnpm-lock.yaml satisfies the package.json dependencies
    /// directive, a headless installation is performed. A headless installation skips all
    /// dependency resolution as it does not need to modify the lockfile.
    #[default = true]
    pub prefer_frozen_lockfile: bool,

    /// When `true`, runtime dependencies (`node@runtime:`,
    /// `deno@runtime:`, `bun@runtime:`) are skipped at install
    /// time — their archives aren't fetched, their slots aren't
    /// materialized, and their bins aren't linked. The rest of
    /// the install proceeds normally. Mirrors pnpm's
    /// [`skipRuntimes`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts)
    /// option, exposed via the `--no-runtime` CLI flag.
    ///
    /// Defaults to `false`, matching upstream. CI scenarios that
    /// pre-provision the runtime (or want to install one runtime
    /// with another pacquet binary) flip this to `true`.
    pub skip_runtimes: bool,

    /// Refuse network requests during install. Mirrors pnpm's
    /// [`offline`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts)
    /// flag — upstream gates the metadata-fetch path with
    /// `ERR_PNPM_NO_OFFLINE_META` when no cached metadata exists for a
    /// spec. Pacquet doesn't have a metadata-fetch path yet (no
    /// resolver until Stage 2), so the same flag instead gates
    /// pacquet's tarball-fetch fall-through: when both the warm
    /// prefetch and the SQLite `index.db` lookup miss, the tarball
    /// fetcher fails fast with `ERR_PACQUET_NO_OFFLINE_TARBALL`
    /// rather than hitting the registry. The frozen-lockfile install
    /// path needs no metadata, so the surface area collapses to
    /// "every snapshot must already be in the local store".
    ///
    /// Pacquet's tarball-side gate has no exact upstream counterpart
    /// (pnpm doesn't gate the tarball fetcher on `offline`), but it's
    /// the most useful interpretation of the flag for a frozen
    /// installer: surface a clear `offline` error rather than letting
    /// the underlying `connection refused` / DNS error propagate.
    /// The Stage 2 resolver will additionally honor the flag on the
    /// metadata path.
    pub offline: bool,

    /// Prefer the local store on read, fall back to the network on a
    /// cache miss. Mirrors pnpm's
    /// [`preferOffline`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts)
    /// flag, which biases the resolver to use cached metadata when
    /// available even past the freshness window.
    ///
    /// Pacquet's frozen-install path already prefers the local store
    /// — the warm prefetch + SQLite-cache lookups always run before
    /// any network fetch — so `prefer_offline` is effectively a no-op
    /// today. The field exists so `.npmrc` / yaml / CLI all parse the
    /// flag cleanly; Stage 2's resolver will honor it the same way
    /// upstream does.
    pub prefer_offline: bool,

    /// Add the full URL to the package's tarball to every entry in pnpm-lock.yaml.
    pub lockfile_include_tarball_url: bool,

    /// The base URL of the npm package registry (trailing slash included).
    #[default(_code = "default_registry()")]
    pub registry: String, // TODO: use Url type (compatible with reqwest)

    /// Resolved proxy configuration — `https-proxy`, `http-proxy`, and
    /// `no-proxy` (plus the legacy `proxy` key and env-var fallbacks),
    /// all from `.npmrc` and the process environment. The type lives
    /// in `pacquet-network` (where it is consumed by
    /// `ThrottledClient::for_installs`) because `pacquet-config`
    /// already depends on `pacquet-network` for auth-headers plumbing.
    /// Default is empty (`None` for every field) — i.e. no proxy.
    pub proxy: pacquet_network::ProxyConfig,

    /// Resolved TLS + `local-address` configuration — `ca`, `cafile`,
    /// `cert`, `key`, `strict-ssl`, `local-address` from `.npmrc`. The
    /// type lives in `pacquet-network` for the same reason as
    /// [`Self::proxy`]. `strict_ssl: None` here means "unset"; the
    /// `true` default is applied at client-build time by
    /// `ThrottledClient::for_installs`, mirroring pnpm's per-emit-site
    /// `strictSsl ?? true` default.
    pub tls: pacquet_network::TlsConfig,

    /// Per-registry TLS overrides — `//host[:port]/path/:ca`,
    /// `:cafile`, `:cert`, `:certfile`, `:key`, `:keyfile` from
    /// `.npmrc`. Lookup uses pnpm's 5-step nerf-darted fallback
    /// chain (exact > nerf-dart > no-port > shorter path prefix >
    /// recursive no-port retry). Per-registry fields override
    /// [`Self::tls`] field-by-field at request time, matching
    /// pnpm's [`{ ...opts, ...sslConfig }`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L143)
    /// spread.
    pub tls_by_uri: pacquet_network::PerRegistryTls,

    /// When true, any missing non-optional peer dependencies are automatically installed.
    #[default = true]
    pub auto_install_peers: bool,

    /// Under `nodeLinker: hoisted`, controls whether non-root
    /// workspace importers are added as children of the virtual
    /// `.` root in the hoist tree. Default `true` matches pnpm —
    /// the whole workspace shares one hoist plan so conflicting
    /// versions across projects dedupe.
    ///
    /// Setting this to `false` opts each project into independent
    /// hoisting (its own subtree, no cross-project dedupe). Niche;
    /// pnpm exposes this knob for the Bit CLI (which lays out its
    /// own root) and for tests. Mirrors upstream's
    /// [`hoistWorkspacePackages`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L51-L66).
    /// No effect under `nodeLinker: isolated` — that linker keeps
    /// per-importer subtrees by construction.
    #[default = true]
    pub hoist_workspace_packages: bool,

    /// Per-importer block-list of package aliases that may NOT be
    /// hoisted past that importer's slot. Outer key is the
    /// importer locator (e.g. `'.@'` for the root project, or the
    /// percent-encoded importer id with the `@` slot suffix);
    /// inner set is the alias names whose hoisting is bordered.
    ///
    /// Programmatic-only upstream — pnpm exposes it through the
    /// embedded API and Bit CLI rather than `pnpm-workspace.yaml`,
    /// because the ergonomics of the locator-keyed map don't
    /// translate cleanly to a yaml setting. Pacquet exposes it
    /// via `HoistOpts::hoisting_limits` (in `pacquet-real-hoist`)
    /// and reads the same yaml shape (`hoistingLimits: { ".@": [...] }`)
    /// for parity.
    ///
    /// Default empty (no aliases bordered). Mirrors upstream's
    /// [`hoistingLimits`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L10).
    /// No effect under `nodeLinker: isolated`.
    pub hoisting_limits: BTreeMap<String, BTreeSet<String>>,

    /// Name slots reserved at the root for an external linker
    /// (the Bit CLI is the only known consumer upstream). Any
    /// dependency whose alias matches one of these names is
    /// stripped from the hoist tree's top-level entries — the
    /// external linker materializes those slots itself.
    ///
    /// Programmatic-only upstream; pacquet exposes the same yaml
    /// shape (`externalDependencies: ["bit-bin"]`) for parity.
    ///
    /// Default empty. Mirrors upstream's
    /// [`externalDependencies`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L18).
    /// No effect under `nodeLinker: isolated`.
    pub external_dependencies: BTreeSet<String>,

    /// When this setting is set to true, packages with peer dependencies will be deduplicated after peers resolution.
    #[default = true]
    pub dedupe_peer_dependents: bool,

    /// If this is enabled, commands will fail if there is a missing or invalid peer dependency in the tree.
    pub strict_peer_dependencies: bool,

    /// When enabled, dependencies of the root workspace project are used to resolve peer
    /// dependencies of any projects in the workspace. It is a useful feature as you can install
    /// your peer dependencies only in the root of the workspace, and you can be sure that all
    /// projects in the workspace use the same versions of the peer dependencies.
    #[default = true]
    pub resolve_peers_from_workspace_root: bool,

    /// Whether to verify each CAFS file's on-disk integrity before reusing it
    /// for an install. When `true` (pnpm's default), the store-index cache
    /// lookup stats each referenced file and re-hashes any whose mtime has
    /// advanced past the stored `checkedAt` timestamp. When `false`, the
    /// lookup skips that verification entirely and trusts the index — a
    /// missing blob is discovered lazily at link time instead.
    ///
    /// Matches pnpm's `verifyStoreIntegrity` camelCase key in
    /// `pnpm-workspace.yaml` (same `true` default as pnpm's
    /// `installing/deps-installer/src/install/extendInstallOptions.ts`).
    #[default = true]
    pub verify_store_integrity: bool,

    /// Whether to consult the side-effects cache
    /// (`PackageFilesIndex.sideEffects`) when importing a package
    /// and whether to populate it after a successful postinstall.
    /// Read from `pnpm-workspace.yaml`'s `sideEffectsCache` field
    /// (camelCase, optional, defaults `true`).
    ///
    /// Default `true`, matching pnpm's `side-effects-cache` at
    /// [`config/reader/src/index.ts`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/config/reader/src/index.ts#L614-L615).
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
    /// the cache after a successful postinstall. Mirrors pnpm's
    /// [`side-effects-cache-readonly`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/config/reader/src/Config.ts#L124).
    /// Default `false`. Read from `pnpm-workspace.yaml`'s
    /// `sideEffectsCacheReadonly` field.
    ///
    /// Consume via [`Config::side_effects_cache_read`] and
    /// [`Config::side_effects_cache_write`].
    pub side_effects_cache_readonly: bool,

    /// How many times pacquet retries a failed tarball fetch on transient
    /// errors before giving up. Mirrors pnpm's `fetchRetries` (default
    /// `2`, matching `config/config/src/index.ts`). The value is the count
    /// of *retries*, so total attempts = `fetch_retries + 1`.
    ///
    /// Today this only gates the `pacquet-tarball` download path;
    /// `crates/registry`'s metadata fetches still issue a single request.
    /// Threading the same retry policy through the registry client is a
    /// follow-up.
    ///
    /// Read from `pnpm-workspace.yaml` only — pnpm 11's
    /// [`isIniConfigKey`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/localConfig.ts#L160-L161)
    /// excludes the `fetch-retry*` family from `NPM_AUTH_SETTINGS`, so a
    /// `fetch-retries=…` line in `.npmrc` is ignored upstream and is
    /// ignored here too.
    #[default(_code = "default_fetch_retries()")]
    pub fetch_retries: u32,

    /// Exponential-backoff growth factor between retry attempts. Mirrors
    /// pnpm's `fetchRetryFactor` (default `10`). Successive backoff is
    /// `min(fetch_retry_mintimeout * factor^attempt, fetch_retry_maxtimeout)`.
    /// Yaml-only — see [`Config::fetch_retries`].
    #[default(_code = "default_fetch_retry_factor()")]
    pub fetch_retry_factor: u32,

    /// Floor in milliseconds for the wait between retries. Mirrors pnpm's
    /// `fetchRetryMintimeout` (default `10000` — 10 s). Yaml-only — see
    /// [`Config::fetch_retries`].
    #[default(_code = "default_fetch_retry_mintimeout()")]
    pub fetch_retry_mintimeout: u64,

    /// Cap in milliseconds on the wait between retries. Mirrors pnpm's
    /// `fetchRetryMaxtimeout` (default `60000` — 1 min). Yaml-only —
    /// see [`Config::fetch_retries`].
    #[default(_code = "default_fetch_retry_maxtimeout()")]
    pub fetch_retry_maxtimeout: u64,

    /// Directory containing the nearest ancestor `pnpm-workspace.yaml`.
    /// Set by [`WorkspaceSettings::apply_to`] when yaml was found, so
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
    /// yaml — matching upstream's JS-object iteration and keeping
    /// `PATCH_KEY_CONFLICT` diagnostics aligned.
    ///
    /// pnpm v11 reads `patchedDependencies` from `pnpm-workspace.yaml`
    /// only — see upstream's
    /// [`addSettingsFromWorkspaceManifestToConfig`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L803-L831).
    pub patched_dependencies: Option<IndexMap<String, String>>,

    /// `pnpm.allowBuilds` from `pnpm-workspace.yaml`: package names
    /// (or `name@version` keys) that are allowed to run lifecycle
    /// scripts. pnpm 11 denies scripts by default; the allow-list is
    /// the opt-in mechanism. Consumed by `AllowBuildPolicy::from_config`
    /// in `pacquet-package-manager`.
    ///
    /// Default empty. Mirrors upstream's
    /// [`createAllowBuildFunction`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/src/index.ts).
    pub allow_builds: HashMap<String, bool>,

    /// `dangerouslyAllowAllBuilds` from `pnpm-workspace.yaml`. When
    /// `true`, every package may run lifecycle scripts regardless of
    /// `allow_builds`. Default `false` to match pnpm v11.
    pub dangerously_allow_all_builds: bool,

    /// `scriptsPrependNodePath` from `pnpm-workspace.yaml`. Controls
    /// whether `dirname(node_execpath)` is prepended to `PATH` when
    /// running lifecycle scripts. Default `Never` to match pnpm's
    /// [`StrictBuildOptions.scriptsPrependNodePath: false`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/after-install/src/extendBuildOptions.ts#L78).
    /// Yaml accepts `true` / `false` / `"warn-only"`.
    pub scripts_prepend_node_path: ScriptsPrependNodePath,

    /// `unsafePerm` from `pnpm-workspace.yaml`. When `false`,
    /// pnpm runs lifecycle scripts under a TMPDIR isolated to
    /// `node_modules/.tmp` and (in upstream) drops uid/gid to a
    /// non-root user. Pacquet honors the TMPDIR side of the
    /// upstream behavior (see `pacquet_executor::make_env`); the
    /// uid/gid drop is a no-op in practice because pnpm's
    /// npm-lifecycle fork never populates `opts.user` /
    /// `opts.group`, so even upstream just re-applies the current
    /// process's uid/gid.
    ///
    /// The default is auto-detected via [`default_unsafe_perm`] to
    /// mirror upstream's [`StrictBuildOptions.unsafePerm`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/extendBuildOptions.ts#L83-L86):
    /// `true` on Windows or POSIX-not-root; `false` when running
    /// as root on POSIX. On Windows,
    /// [`WorkspaceSettings::apply_to`] also force-overrides the
    /// applied value to `true` regardless of yaml — matching
    /// upstream's `process.platform === 'win32'` gate at
    /// [`@pnpm/npm-lifecycle/index.js:204-220`](https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L204-L220).
    #[default(_code = "default_unsafe_perm()")]
    pub unsafe_perm: bool,

    /// `childConcurrency` from `pnpm-workspace.yaml` — the maximum
    /// number of lifecycle-script spawns that may run in parallel
    /// inside a single `BuildModules` chunk. Resolved through
    /// [`resolve_child_concurrency`] so the yaml value can be
    /// negative (interpreted as `parallelism - |value|`).
    ///
    /// Default: `min(4, availableParallelism())`, matching upstream's
    /// [`getDefaultWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L21-L23).
    /// Chunks run sequentially (children before parents); only
    /// members within a chunk are parallelized — same as upstream's
    /// [`runGroups(getWorkspaceConcurrency(opts.childConcurrency), groups)`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L124).
    #[default(_code = "default_child_concurrency()")]
    pub child_concurrency: u32,

    /// Git host names where pacquet should clone via `git init` +
    /// `git remote add` + `git fetch --depth 1 origin <commit>` instead
    /// of a full `git clone`. Saves bandwidth and disk when the remote
    /// only needs the pinned commit. Mirrors pnpm's `gitShallowHosts`
    /// default at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L155-L162>.
    ///
    /// The default list follows
    /// <https://github.com/npm/git/blob/1e1dbd26bd/lib/clone.js#L13-L19>.
    #[default(_code = "default_git_shallow_hosts()")]
    pub git_shallow_hosts: Vec<String>,

    /// `supportedArchitectures` from `pnpm-workspace.yaml`. Threaded
    /// into the installability check at install time (via
    /// `pacquet-package-manager`'s `InstallabilityHost`, downstream of
    /// this crate) so optional platform-tagged dependencies for the
    /// listed `os` / `cpu` / `libc` values are kept even when they
    /// don't match the host triple. Per-axis CLI flags (`--cpu`,
    /// `--libc`, `--os`) override individual axes — mirrors upstream's
    /// [`overrideSupportedArchitecturesWithCLI`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/overrideSupportedArchitecturesWithCLI.ts).
    /// Default `None` so the host triple is the sole accept set
    /// (matches upstream's behavior when neither yaml nor CLI sets a
    /// value).
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,

    /// `ignoredOptionalDependencies` from `pnpm-workspace.yaml`. A
    /// list of dep-name patterns the user wants entirely excluded
    /// from resolution + install. At manifest read time each
    /// matching key is dropped from `optionalDependencies` AND from
    /// `dependencies` (a package may list the same dep under both
    /// to make it optional only for some installers). Mirrors
    /// upstream's
    /// [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts).
    ///
    /// The resolved set is also recorded on the lockfile so a
    /// subsequent install can detect drift between
    /// `pnpm-workspace.yaml` and the lockfile-recorded set —
    /// mismatch triggers `OutdatedLockfile`. Mirrors upstream's
    /// drift check at
    /// [`getOutdatedLockfileSetting.ts:58-60`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L58-L60).
    pub ignored_optional_dependencies: Option<Vec<String>>,

    /// Per-registry `Authorization` header lookup, populated from
    /// `.npmrc` auth keys (`_auth`, `_authToken`, `username`/`_password`,
    /// scoped variants). Threaded through the network and tarball
    /// fetchers via [`pacquet_network::AuthHeaders::for_url`]. Empty
    /// when no `.npmrc` was found or no auth keys were set.
    pub auth_headers: std::sync::Arc<pacquet_network::AuthHeaders>,
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the install should consult the side-effects cache.
    /// Mirrors upstream's
    /// [`sideEffectsCacheRead = sideEffectsCache ?? sideEffectsCacheReadonly`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/config/reader/src/index.ts#L614).
    ///
    /// Pacquet collapses upstream's tri-state (`undefined`/`true`/`false`)
    /// into two booleans: the cache is read when either flag is on, so
    /// users who only want the READ side can set
    /// `sideEffectsCacheReadonly: true` with `sideEffectsCache: false`
    /// and get a read-only view.
    pub fn side_effects_cache_read(&self) -> bool {
        self.side_effects_cache || self.side_effects_cache_readonly
    }

    /// Whether the install is allowed to populate the side-effects
    /// cache after a successful postinstall. Mirrors upstream's
    /// [`sideEffectsCacheWrite = sideEffectsCache`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/config/reader/src/index.ts#L615)
    /// with the additional constraint that the explicit
    /// `sideEffectsCacheReadonly: true` always wins — upstream's
    /// `??` semantics let `readonly` slip through when both flags
    /// are explicitly set, but `readonly` as a flag name only makes
    /// sense if it really does block writes.
    pub fn side_effects_cache_write(&self) -> bool {
        self.side_effects_cache && !self.side_effects_cache_readonly
    }

    /// Resolve relative patch file paths in
    /// [`Config::patched_dependencies`] against
    /// [`Config::workspace_dir`], compute SHA-256 hashes, and bucket
    /// the entries into a [`PatchGroupRecord`].
    ///
    /// Mirrors the workspace-dir half of upstream's
    /// [`getOptionsFromPnpmSettings`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/getOptionsFromRootManifest.ts#L28-L46)
    /// composed with the
    /// [`calcPatchHashes` step](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/src/install/index.ts#L468-L488).
    ///
    /// Returns `Ok(None)` when either field is unset (no yaml
    /// found or no `patchedDependencies` key). Returns `Err(_)`
    /// when any patch file can't be hashed or any key has an
    /// invalid semver range.
    ///
    /// IO-heavy; call once per install rather than at every site
    /// that needs the resolved record.
    /// Derive [`Self::global_virtual_store_dir`] from
    /// `enable_global_virtual_store` + the existing `store_dir` /
    /// `virtual_store_dir` fields.
    ///
    /// Pacquet diverges from upstream's
    /// [`extendInstallOptions.ts:343-355`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/extendInstallOptions.ts#L343-L355)
    /// on *which* field carries the GVS path:
    ///
    /// - **Upstream**: mutates `virtualStoreDir` in place when GVS is
    ///   on and the user hasn't pinned it, so every consumer that
    ///   reads `virtualStoreDir` ends up looking at `<storeDir>/links`.
    /// - **Pacquet**: keeps `virtual_store_dir` at its project-local
    ///   value (`<cwd>/node_modules/.pnpm` by default, or the user's
    ///   yaml-pinned path) and writes the GVS path into the separate
    ///   `global_virtual_store_dir` field. The install layer picks the
    ///   right field through [`crate::Config::enable_global_virtual_store`]
    ///   (or, in practice, through `pacquet_package_manager::VirtualStoreLayout`).
    ///
    /// The reason: pacquet still has a non-frozen
    /// `InstallWithoutLockfile` path that upstream pnpm doesn't have.
    /// Mutating `virtual_store_dir` would redirect that path to
    /// `<storeDir>/links` too — but the issue (pnpm/pacquet#432)
    /// scopes GVS to frozen-lockfile installs. Splitting the field
    /// keeps the without-lockfile path on the project-local layout
    /// while the frozen-lockfile path consumes the GVS-derived value.
    ///
    /// `virtual_store_dir_explicit` carries the "did the user set
    /// `virtualStoreDir` in yaml" signal `SmartDefault` cannot express
    /// on its own. When `true` *and* GVS is on, `global_virtual_store_dir`
    /// mirrors `virtual_store_dir` (the user picked the GVS root via the
    /// shared key). `global_virtual_store_dir_explicit` is the analogous
    /// signal for the dedicated `globalVirtualStoreDir` yaml key — when
    /// set, that value wins and the derivation leaves
    /// `global_virtual_store_dir` alone. Otherwise the field falls back
    /// to `<store_dir>/links`, mirroring upstream's unconditional
    /// `globalVirtualStoreDir = storeDir/links` assignment for the unset
    /// case.
    pub fn apply_global_virtual_store_derivation(
        &mut self,
        virtual_store_dir_explicit: bool,
        global_virtual_store_dir_explicit: bool,
    ) {
        if global_virtual_store_dir_explicit {
            // User pinned the dedicated GVS key in yaml — honor it.
            return;
        }
        self.global_virtual_store_dir =
            if self.enable_global_virtual_store && virtual_store_dir_explicit {
                self.virtual_store_dir.clone()
            } else {
                self.store_dir.links()
            };
    }

    pub fn resolved_patched_dependencies(
        &self,
    ) -> Result<Option<PatchGroupRecord>, ResolvePatchedDependenciesError> {
        let (Some(workspace_dir), Some(raw)) = (&self.workspace_dir, &self.patched_dependencies)
        else {
            return Ok(None);
        };
        resolve_and_group(workspace_dir, raw)
    }

    /// Build the runtime config by layering:
    /// 1. hard-coded defaults, then
    /// 2. the supported `.npmrc` subset read from the nearest `.npmrc`
    ///    (cwd, falling back to home), then
    /// 3. the nearest `pnpm-workspace.yaml` walking up from cwd.
    ///
    /// Pacquet currently applies `registry`, npm-auth credentials, the
    /// proxy keys (`https-proxy`, `http-proxy`, `proxy`, `no-proxy` /
    /// `noproxy`), and the TLS + local-address keys (`ca`, `cafile`,
    /// `cert`, `key`, `strict-ssl`, `local-address`) from `.npmrc`.
    /// Other `.npmrc` entries — pnpm's scoped-registry keys and
    /// per-registry TLS overrides (`//host:cafile=`, `//host:ca=`,
    /// `//host:cert=`, `//host:key=`), plus project-structural
    /// settings like `storeDir`, `lockfile` and `hoist-pattern` — are
    /// silently ignored here. The first group is tracked for future
    /// per-registry-TLS work; the second must come from
    /// `pnpm-workspace.yaml` or CLI flags, matching pnpm 11.
    ///
    /// The yaml wins over `.npmrc` on any key it sets.
    ///
    /// Returns [`LoadWorkspaceYamlError`] when an existing
    /// `pnpm-workspace.yaml` cannot be read or parsed, matching pnpm's
    /// [`readWorkspaceManifest`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/workspace-manifest-reader/src/index.ts).
    /// A missing file is not an error.
    pub fn current<Sys, Error, CurrentDir, HomeDir, Default>(
        current_dir: CurrentDir,
        home_dir: HomeDir,
        default: Default,
    ) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar,
        CurrentDir: FnOnce() -> Result<PathBuf, Error>,
        HomeDir: FnOnce() -> Option<PathBuf>,
        Default: FnOnce() -> Config,
    {
        let mut config = default();

        let cwd = current_dir().ok();
        // Re-anchor the path-valued defaults (`modules_dir`,
        // `virtual_store_dir`) onto the caller-supplied cwd. SmartDefault
        // populates them via [`defaults::default_modules_dir`] /
        // [`defaults::default_virtual_store_dir`], which both anchor at
        // `env::current_dir()`. That diverges from `cwd` whenever the
        // caller passed a different directory (notably
        // `pacquet --dir <path>` from elsewhere), so without this fixup
        // pacquet would load config from `<path>` while still installing
        // to the process-cwd `node_modules`. Matches pnpm 11, whose
        // `modulesDir`/`virtualStoreDir` defaults are resolved against
        // `pnpmConfig.dir`.
        if let Some(start) = &cwd {
            config.modules_dir = start.join("node_modules");
            config.virtual_store_dir = start.join("node_modules/.pnpm");
        }

        // Read the nearest .npmrc (cwd first, home second) and apply only
        // the auth/network subset. Everything else is intentionally ignored.
        //
        // Two-phase apply: write the resolved `registry` (and emit any
        // ${VAR}-substitution warnings) *before* layering
        // `pnpm-workspace.yaml`, then build `auth_headers` *after* yaml has
        // had a chance to override `registry`. Pnpm keys default-registry
        // creds at the final resolved URL, not the `.npmrc` literal — see
        // [`getAuthHeadersFromConfig`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/getAuthHeadersFromConfig.ts).
        let auth_source = cwd
            .as_ref()
            .and_then(|dir| read_npmrc(dir))
            .or_else(|| home_dir().and_then(|dir| read_npmrc(&dir)));
        let mut npmrc_auth = auth_source
            .map(|text| crate::npmrc_auth::NpmrcAuth::from_ini::<Sys>(&text))
            .unwrap_or_default();
        npmrc_auth.apply_registry_and_warn(&mut config);
        // Proxy cascade fires unconditionally — even when no `.npmrc`
        // is found — because the env-var fallback in pnpm's
        // [`config/reader/src/index.ts:591-600`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L591-L600)
        // is a normalization step on the resolved config, not a
        // function of `.npmrc` presence.
        npmrc_auth.apply_proxy_cascade::<Sys>(&mut config);
        // TLS + local-address are sourced from `.npmrc` only — pnpm
        // does not honor env vars (`NODE_EXTRA_CA_CERTS`,
        // `NODE_TLS_REJECT_UNAUTHORIZED`, etc.) for these keys
        // (Node's runtime does, but pnpm's reader does not). When
        // there is no `.npmrc`, `npmrc_auth` is the default value and
        // this is a no-op write of `TlsConfig::default()` onto the
        // already-default `config.tls`.
        npmrc_auth.apply_tls_and_local_address(&mut config);

        // Layer pnpm-workspace.yaml overrides on top. A missing file is
        // silent. Read or parse failures propagate to the caller.
        //
        // Resolve the workspace dir: `NPM_CONFIG_WORKSPACE_DIR`
        // override first (mirroring upstream's `findWorkspaceDir` and
        // [`pacquet_workspace::find_workspace_dir`]; both must agree on
        // where the workspace lives, otherwise the per-importer
        // `SymlinkDirectDependencies` writes and the virtual store
        // would end up in different directories). Fall back to the
        // upward walk for `pnpm-workspace.yaml` when the env var is
        // unset or empty.
        //
        // The env var is read here rather than via
        // [`pacquet_workspace`] to avoid adding a cross-crate
        // dependency just for the lookup — the contract is fixed by
        // pnpm upstream, so the duplication is low-risk.
        //
        // Capture the "did yaml set this field" booleans *before*
        // applying yaml so the GVS derivation downstream can tell apart
        // user-pinned values from SmartDefault fallbacks. Without these
        // signals the derivation would always see populated values
        // (SmartDefault wrote them in) and would either always or never
        // re-point them, neither of which matches upstream's
        // [`extendInstallOptions.ts:343-355`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/extendInstallOptions.ts#L343-L355).
        let env_workspace_dir = std::env::var_os("NPM_CONFIG_WORKSPACE_DIR")
            .or_else(|| std::env::var_os("npm_config_workspace_dir"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let workspace_yaml = if let Some(env_dir) = env_workspace_dir {
            // Env-var path: load yaml directly from the env dir. A
            // missing file is silent (matching upstream), but the
            // re-anchor still fires because the user has explicitly
            // told us where the workspace lives.
            let yaml_path = env_dir.join(WORKSPACE_MANIFEST_FILENAME);
            match fs::read_to_string(&yaml_path) {
                Ok(text) => {
                    let settings: WorkspaceSettings =
                        serde_saphyr::from_str(&text).map_err(Box::new).map_err(|source| {
                            LoadWorkspaceYamlError::ParseYaml { path: yaml_path, source }
                        })?;
                    Some((env_dir, Some(settings)))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some((env_dir, None)),
                Err(source) => {
                    return Err(LoadWorkspaceYamlError::ReadFile { path: yaml_path, source });
                }
            }
        } else if let Some(start) = &cwd {
            WorkspaceSettings::find_and_load(start)?.map(|(path, settings)| {
                let base_dir = path.parent().unwrap_or(start).to_path_buf();
                (base_dir, Some(settings))
            })
        } else {
            None
        };

        let mut virtual_store_dir_explicit = false;
        let mut global_virtual_store_dir_explicit = false;
        if let Some((base_dir, settings)) = workspace_yaml {
            // Re-anchor the path-valued defaults to the workspace root
            // before applying settings. Without this, a `pacquet install`
            // run from a workspace subdirectory leaves
            // `modules_dir` / `virtual_store_dir` anchored at the CLI
            // `--dir` (the subdir), while the per-importer
            // [`SymlinkDirectDependencies`] writes are anchored at the
            // workspace root — producing two `node_modules` layouts
            // for the same install. pnpm v11 ties
            // `pnpmConfig.dir = lockfileDir` exactly so its defaults
            // resolve from the workspace root; we mirror that here.
            //
            // Applied *before* `settings.apply_to` so an explicit
            // `modulesDir` / `virtualStoreDir` in `pnpm-workspace.yaml`
            // still wins.
            config.modules_dir = base_dir.join("node_modules");
            config.virtual_store_dir = base_dir.join("node_modules/.pnpm");
            if let Some(settings) = settings {
                virtual_store_dir_explicit = settings.virtual_store_dir.is_some();
                global_virtual_store_dir_explicit = settings.global_virtual_store_dir.is_some();
                settings.apply_to(&mut config, &base_dir);
            }
        }

        // Now that `registry` has been finalised (yaml may have
        // overridden the `.npmrc` value), build the per-URI auth
        // header lookup so default-registry creds key at the final
        // URL.
        npmrc_auth.build_auth_headers(&mut config);

        // Derive `global_virtual_store_dir` last so it sees the final
        // `store_dir` / `virtual_store_dir` after yaml has been
        // applied. An explicit `globalVirtualStoreDir` in yaml wins
        // over the derivation; otherwise the field falls back to the
        // user's pinned `virtualStoreDir` (under GVS-on) or to
        // `<store_dir>/links`. See
        // [`Self::apply_global_virtual_store_derivation`].
        config.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );

        Ok(config)
    }

    /// Persist the config data until the program terminates.
    pub fn leak(self) -> &'static mut Self {
        self.pipe(Box::new).pipe(Box::leak)
    }
}

/// Read the text of the `.npmrc` in `dir`, returning `None` for anything
/// from "file doesn't exist" to "not valid UTF-8" — same best-effort
/// behaviour as pnpm. The caller decides which keys to honour.
fn read_npmrc(dir: &std::path::Path) -> Option<String> {
    fs::read_to_string(dir.join(".npmrc")).ok()
}

#[cfg(test)]
mod tests {
    use std::env;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::{Config, EnvVar, Host, NodeLinker, PackageImportMethod, fs};
    use crate::defaults::default_store_dir;
    use pacquet_store_dir::StoreDir;
    use pacquet_testing_utils::env_guard::EnvGuard;
    use pipe_trait::Pipe;

    fn display_store_dir(store_dir: &StoreDir) -> String {
        store_dir.display().to_string().replace('\\', "/")
    }

    #[test]
    pub fn have_default_values() {
        let value = Config::new();
        assert_eq!(value.node_linker, NodeLinker::default());
        assert_eq!(value.package_import_method, PackageImportMethod::default());
        assert!(value.prefer_frozen_lockfile);
        assert!(value.symlink);
        assert!(value.hoist);
        // The SmartDefault expression for `store_dir` resolves to
        // `default_store_dir::<Host>(home::home_dir, env::current_dir)`
        // via the thin `default_store_dir_host` wrapper, so calling
        // the generic helper here with the same `Host` capability and
        // the same OS closures must produce the same value — even on
        // a developer machine with `PNPM_HOME` / `XDG_DATA_HOME` set.
        // This is the wiring assertion that proves the SmartDefault
        // field still goes through the production capability; the
        // per-branch behaviour of `default_store_dir` is exercised
        // with fake-`Sys` structs in `defaults::tests`.
        assert_eq!(
            value.store_dir,
            default_store_dir::<Host, _, _, _>(home::home_dir, env::current_dir),
        );
        assert_eq!(value.registry, "https://registry.npmjs.org/");
    }

    /// `fetch-retries*` defaults must match pnpm's
    /// `config/config/src/index.ts` (`2`, `10`, `10000`, `60000`) — these
    /// are the values pnpm bakes into npm-style fetches and we want
    /// pacquet to behave identically out of the box.
    #[test]
    pub fn fetch_retries_defaults_match_pnpm() {
        let value = Config::new();
        assert_eq!(value.fetch_retries, 2);
        assert_eq!(value.fetch_retry_factor, 10);
        assert_eq!(value.fetch_retry_mintimeout, 10_000);
        assert_eq!(value.fetch_retry_maxtimeout, 60_000);
    }

    /// `default_store_dir`'s `PNPM_HOME` branch, exercised through the
    /// generic capability seam — no process-environment mutation, no
    /// `EnvGuard` lock, no `unsafe` block. The earlier shape of this
    /// test set `PNPM_HOME` via `std::env::set_var` and called
    /// `Config::new()`; with the DI seam from pnpm/pacquet#339 +
    /// pnpm/pnpm#11708 the same precedence is checked by passing a
    /// per-test unit struct that satisfies [`EnvVar`].
    ///
    /// The `home_dir` and `current_dir` closures both call
    /// `unreachable!` because `default_store_dir` short-circuits on
    /// `PNPM_HOME` before consulting either — the panic-on-call
    /// documents that precondition. Tracks pnpm/pacquet#343.
    #[test]
    pub fn should_use_pnpm_home_env_var() {
        struct EnvWithPnpmHome;
        impl EnvVar for EnvWithPnpmHome {
            fn var(name: &str) -> Option<String> {
                (name == "PNPM_HOME").then(|| "/hello".to_owned())
            }
        }
        let store_dir = default_store_dir::<EnvWithPnpmHome, _, _, std::io::Error>(
            || unreachable!("home_dir must not be called when PNPM_HOME is set"),
            || unreachable!("current_dir must not be called when PNPM_HOME is set"),
        );
        assert_eq!(display_store_dir(&store_dir), "/hello/store");
    }

    /// Companion to [`should_use_pnpm_home_env_var`]: when
    /// `PNPM_HOME` is unset, `default_store_dir` falls through to
    /// `XDG_DATA_HOME`. Exercised through the DI seam with a fake
    /// `Sys` that only returns a value for `XDG_DATA_HOME`. No
    /// process-environment mutation, no `EnvGuard`, no `unsafe`.
    /// Tracks pnpm/pacquet#343.
    #[test]
    pub fn should_use_xdg_data_home_env_var() {
        struct EnvWithXdgDataHome;
        impl EnvVar for EnvWithXdgDataHome {
            fn var(name: &str) -> Option<String> {
                (name == "XDG_DATA_HOME").then(|| "/hello".to_owned())
            }
        }
        let store_dir = default_store_dir::<EnvWithXdgDataHome, _, _, std::io::Error>(
            || unreachable!("home_dir must not be called when XDG_DATA_HOME is set"),
            || unreachable!("current_dir must not be called when XDG_DATA_HOME is set"),
        );
        assert_eq!(display_store_dir(&store_dir), "/hello/pnpm/store");
    }

    #[test]
    pub fn npmrc_in_current_folder_applies_registry() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join(".npmrc"), "registry=https://cwd.example")
            .expect("write to .npmrc");
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || unreachable!("shouldn't reach home dir"),
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert_eq!(config.registry, "https://cwd.example/");
    }

    #[test]
    pub fn non_auth_keys_in_npmrc_are_ignored() {
        // pnpm 11 stopped reading project-structural settings from .npmrc.
        // Writing `symlink=false` / `lockfile=true` / hoist / node-linker /
        // store-dir to .npmrc should have no effect on the resolved config.
        let tmp = tempdir().unwrap();
        let non_auth_ini = "symlink=false\nlockfile=true\nhoist=false\nnode-linker=hoisted\n";
        fs::write(tmp.path().join(".npmrc"), non_auth_ini).expect("write to .npmrc");
        let defaults = Config::new();
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert_eq!(config.symlink, defaults.symlink);
        assert_eq!(config.lockfile, defaults.lockfile);
        assert_eq!(config.hoist, defaults.hoist);
        assert_eq!(config.node_linker, defaults.node_linker);
    }

    /// pnpm 11's `isIniConfigKey` (config/config/src/auth.ts) leaves the
    /// `fetch-retries*` family out of `NPM_AUTH_SETTINGS`, so a value
    /// like `fetch-retries=99` in `.npmrc` is silently ignored upstream.
    /// pacquet must do the same — applying it would diverge from pnpm
    /// and silently change install behaviour for projects that have a
    /// stale `.npmrc` lying around.
    #[test]
    pub fn fetch_retry_keys_in_npmrc_are_ignored() {
        let tmp = tempdir().unwrap();
        let ini = "fetch-retries=99\nfetch-retry-factor=99\nfetch-retry-mintimeout=99\nfetch-retry-maxtimeout=99\n";
        fs::write(tmp.path().join(".npmrc"), ini).expect("write to .npmrc");
        let defaults = Config::new();
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert_eq!(config.fetch_retries, defaults.fetch_retries);
        assert_eq!(config.fetch_retry_factor, defaults.fetch_retry_factor);
        assert_eq!(config.fetch_retry_mintimeout, defaults.fetch_retry_mintimeout);
        assert_eq!(config.fetch_retry_maxtimeout, defaults.fetch_retry_maxtimeout);
    }

    #[test]
    pub fn test_current_folder_for_invalid_npmrc() {
        let tmp = tempdir().unwrap();
        // write invalid utf-8 value to npmrc
        fs::write(tmp.path().join(".npmrc"), b"Hello \xff World").expect("write to .npmrc");
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert!(config.symlink); // default — invalid .npmrc is silently ignored
    }

    #[test]
    pub fn npmrc_in_home_folder_applies_registry() {
        let current_dir = tempdir().unwrap();
        let home_dir = tempdir().unwrap();
        fs::write(home_dir.path().join(".npmrc"), "registry=https://home.example")
            .expect("write to .npmrc");
        let config = Config::current::<Host, _, _, _, _>(
            || current_dir.path().to_path_buf().pipe(Ok::<_, ()>),
            || home_dir.path().to_path_buf().pipe(Some),
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert_eq!(config.registry, "https://home.example/");
    }

    #[test]
    pub fn pnpm_workspace_yaml_registry_overrides_npmrc_registry() {
        // `registry` is the one non-scope key pnpm 11 still reads from
        // .npmrc (it's in RAW_AUTH_CFG_KEYS). When both files define it,
        // the yaml wins, matching pnpm itself.
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join(".npmrc"), "registry=https://from-npmrc.test")
            .expect("write to .npmrc");
        fs::write(tmp.path().join("pnpm-workspace.yaml"), "registry: https://from-yaml.test\n")
            .expect("write to pnpm-workspace.yaml");
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || unreachable!("shouldn't reach home dir"),
            Config::new,
        )
        .expect("yaml is valid");
        assert_eq!(config.registry, "https://from-yaml.test/");
    }

    #[test]
    pub fn pnpm_workspace_yaml_found_by_walking_up() {
        let tmp = tempdir().unwrap();
        let nested = tmp.path().join("packages/inner");
        fs::create_dir_all(&nested).unwrap();
        fs::write(tmp.path().join("pnpm-workspace.yaml"), "symlink: false\n")
            .expect("write to pnpm-workspace.yaml");
        // No `.npmrc` anywhere, but a parent dir has `pnpm-workspace.yaml` —
        // the yaml should still be applied.
        let config = Config::current::<Host, _, _, _, _>(
            || nested.clone().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("yaml is valid");
        assert!(!config.symlink);
    }

    #[test]
    pub fn test_current_folder_fallback_to_default() {
        let current_dir = tempdir().unwrap();
        let home_dir = tempdir().unwrap();
        let config = Config::current::<Host, _, _, _, _>(
            || current_dir.path().to_path_buf().pipe(Ok::<_, ()>),
            || home_dir.path().to_path_buf().pipe(Some),
            || Config { symlink: false, ..Config::new() },
        )
        .expect("workspace yaml absent => no error");
        assert!(!config.symlink);
    }

    /// `enableGlobalVirtualStore` defaults to `false` — matches pnpm
    /// v11's effective default for regular installs (the `true`
    /// assignment lives only inside the `--global` install branch;
    /// see [`Config::enable_global_virtual_store`]). The derivation
    /// still fires automatically from [`Config::current`] after yaml
    /// has been applied, writing `<store_dir>/links` into
    /// `global_virtual_store_dir` while leaving `virtual_store_dir`
    /// at its project-local default — both fields stay valid so the
    /// downstream code can read either one without first checking
    /// the toggle. Pacquet's split-field variant of upstream's
    /// [`extendInstallOptions.ts:343-355`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/extendInstallOptions.ts#L343-L355);
    /// see [`Config::apply_global_virtual_store_derivation`] for why
    /// pacquet keeps them separate.
    #[test]
    pub fn gvs_default_is_off_and_paths_derive_cleanly() {
        let tmp = tempdir().unwrap();
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert!(
            !config.enable_global_virtual_store,
            "GVS defaults to false (matches pnpm v11 for non-global installs)",
        );
        // `virtual_store_dir` stays project-local. The
        // `<cwd>/node_modules/.pnpm` default has been re-anchored to
        // `tmp` by `Config::current` (see the cwd fixup block).
        assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
        assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
    }

    /// When `enableGlobalVirtualStore: false`, the derivation still
    /// populates `global_virtual_store_dir` at `<storeDir>/links` —
    /// matching upstream's unconditional `globalVirtualStoreDir =
    /// storeDir/links` assignment for the GVS-off branch. The frozen-
    /// lockfile install path consults `enable_global_virtual_store`
    /// to decide whether to consume the field.
    #[test]
    pub fn gvs_disabled_keeps_project_local_virtual_store() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("pnpm-workspace.yaml"), "enableGlobalVirtualStore: false\n")
            .expect("write to pnpm-workspace.yaml");
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("yaml is valid");
        assert!(!config.enable_global_virtual_store);
        assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
        assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
    }

    /// When the user pins `virtualStoreDir` *and* opts into GVS,
    /// `globalVirtualStoreDir` mirrors that path — the user gets to
    /// pick where the shared store lives. `virtual_store_dir` itself
    /// still holds the pinned value (it's the same field the user
    /// configured).
    #[test]
    pub fn gvs_user_pinned_virtual_store_routes_into_global_virtual_store_dir() {
        let tmp = tempdir().unwrap();
        let user_path = tmp.path().join("custom-links");
        fs::write(
            tmp.path().join("pnpm-workspace.yaml"),
            format!("enableGlobalVirtualStore: true\nvirtualStoreDir: {}\n", user_path.display()),
        )
        .expect("write to pnpm-workspace.yaml");
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("yaml is valid");
        assert!(config.enable_global_virtual_store);
        assert_eq!(config.virtual_store_dir, user_path);
        assert_eq!(config.global_virtual_store_dir, user_path);
    }

    /// An explicit `globalVirtualStoreDir` in yaml wins over the
    /// derivation: the resolved field equals the user-supplied path,
    /// not `<store_dir>/links` and not the user's `virtualStoreDir`.
    /// Mirrors upstream's
    /// [`getOptionsFromRootManifest.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getOptionsFromRootManifest.ts)
    /// — `globalVirtualStoreDir` is read into the config there with
    /// the same resolve-relative-to-workspace semantics. Without this
    /// preservation the value parses from yaml and then gets
    /// silently overwritten by the derivation.
    ///
    /// The fixture also enables GVS explicitly. Pacquet's default
    /// is `enableGlobalVirtualStore: false` (matches pnpm v11 for
    /// non-`--global` installs), so without the explicit opt-in the
    /// GVS-on derivation path wouldn't run at all and the test
    /// would say nothing about that path's behaviour.
    #[test]
    pub fn yaml_global_virtual_store_dir_wins_over_derivation() {
        let tmp = tempdir().unwrap();
        let yaml_gvs = tmp.path().join("my-shared-store");
        fs::write(
            tmp.path().join("pnpm-workspace.yaml"),
            format!(
                "enableGlobalVirtualStore: true\nglobalVirtualStoreDir: {}\n",
                yaml_gvs.display(),
            ),
        )
        .expect("write to pnpm-workspace.yaml");
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("yaml is valid");
        assert!(config.enable_global_virtual_store);
        // `virtual_store_dir` stays at the project-local default,
        // because the user didn't pin `virtualStoreDir`.
        assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
        // The explicit yaml value wins — neither the derivation's
        // `<store_dir>/links` fallback nor any mirroring of
        // `virtual_store_dir` clobbers it.
        assert_eq!(config.global_virtual_store_dir, yaml_gvs);
    }

    /// Real-process-environment smoke test for the proxy env-var
    /// fallback through `Config::current`. The injected-`EnvVar` tests
    /// in `npmrc_auth/tests.rs` cover the cascade branches
    /// exhaustively; this one only proves the wiring through
    /// `Host::var` reaches `std::env::var` and that the cascade
    /// fires even with no `.npmrc` present.
    #[test]
    pub fn proxy_env_fallback_applies_through_current() {
        // Snapshot every proxy var the cascade might read so peer tests
        // can't observe our mutations and so the env restores cleanly.
        let _g = EnvGuard::snapshot([
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "PROXY",
            "proxy",
            "NO_PROXY",
            "no_proxy",
            "NPM_CONFIG_WORKSPACE_DIR",
            "npm_config_workspace_dir",
        ]);
        let tmp = tempdir().unwrap();
        // SAFETY: EnvGuard above serializes the test against other
        // env-mutating tests in this process; no other thread reads
        // these vars concurrently. The other proxy vars are removed so
        // a host-set value can't leak in and skew the assertion.
        unsafe {
            env::remove_var("HTTPS_PROXY");
            env::remove_var("https_proxy");
            env::remove_var("HTTP_PROXY");
            env::remove_var("http_proxy");
            env::remove_var("PROXY");
            env::remove_var("proxy");
            env::remove_var("NO_PROXY");
            env::remove_var("no_proxy");
            env::remove_var("NPM_CONFIG_WORKSPACE_DIR");
            env::remove_var("npm_config_workspace_dir");
            env::set_var("HTTPS_PROXY", "http://env.example:8080");
        }
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("workspace yaml absent => no error");
        assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://env.example:8080"));
        assert_eq!(
            config.proxy.http_proxy.as_deref(),
            Some("http://env.example:8080"),
            "http side cascades through resolved https",
        );
    }

    /// Pnpm's
    /// [`workspace-manifest-reader`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/workspace-manifest-reader/src/index.ts)
    /// fails the process on invalid yaml. `Config::current` must do the
    /// same instead of silently falling back to defaults.
    #[test]
    pub fn invalid_workspace_yaml_propagates_error() {
        let tmp = tempdir().unwrap();
        // `: : :` is rejected by saphyr.
        fs::write(tmp.path().join("pnpm-workspace.yaml"), ": : :\n")
            .expect("write to pnpm-workspace.yaml");
        let result = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        );
        let err = result.expect_err("invalid yaml should error");
        assert!(
            matches!(err, crate::LoadWorkspaceYamlError::ParseYaml { .. }),
            "expected ParseYaml, got {err:?}",
        );
    }

    /// Running `pacquet install` from a workspace subdirectory must
    /// not leave `modules_dir` / `virtual_store_dir` anchored at the
    /// CLI `--dir`. The presence of `pnpm-workspace.yaml` in an
    /// ancestor signals that the workspace root is the install anchor,
    /// matching pnpm v11's `pnpmConfig.dir = lockfileDir` rule. Without
    /// this, the per-importer `node_modules` writes (under the
    /// workspace root) and the virtual store (under the subdir) would
    /// produce two inconsistent layouts for the same install.
    #[test]
    pub fn workspace_subdir_anchors_modules_at_workspace_root() {
        let tmp = tempdir().unwrap();
        let workspace_root = tmp.path();
        let subdir = workspace_root.join("packages/web");
        fs::create_dir_all(&subdir).expect("create subdir");
        fs::write(workspace_root.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
            .expect("write to pnpm-workspace.yaml");

        let config = Config::current::<Host, _, _, _, _>(
            || subdir.clone().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("config loads");

        assert_eq!(
            config.modules_dir,
            workspace_root.join("node_modules"),
            "modules_dir must be anchored at the workspace root, not the subdir",
        );
        assert_eq!(
            config.virtual_store_dir,
            workspace_root.join("node_modules/.pnpm"),
            "virtual_store_dir must be anchored at the workspace root, not the subdir",
        );
    }

    /// A single-project install (no `pnpm-workspace.yaml` anywhere)
    /// keeps the CLI `--dir` as the anchor. Guards against the
    /// re-anchor block accidentally firing when no workspace exists.
    #[test]
    pub fn single_project_anchors_modules_at_cwd() {
        // Even though this test doesn't `set_var`, hold the env
        // guard so a *concurrent* `NPM_CONFIG_WORKSPACE_DIR` test
        // can't make this one fall into the env-var override path.
        let _guard = EnvGuard::snapshot(["NPM_CONFIG_WORKSPACE_DIR", "npm_config_workspace_dir"]);
        // SAFETY: lock held by `_guard`. Two removes are fine on
        // both POSIX (case-sensitive: two distinct vars) and Windows
        // (case-insensitive: the second remove is a no-op on an
        // already-absent variable).
        unsafe {
            env::remove_var("NPM_CONFIG_WORKSPACE_DIR");
            env::remove_var("npm_config_workspace_dir");
        }
        let tmp = tempdir().unwrap();
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("config loads");
        assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
        assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    }

    /// `NPM_CONFIG_WORKSPACE_DIR` must steer `Config::current`'s
    /// path-anchoring just like it steers
    /// [`pacquet_workspace::find_workspace_dir`] — otherwise the
    /// virtual store would land in the cwd while the per-importer
    /// `SymlinkDirectDependencies` writes land under the env-var
    /// path, producing two `node_modules` layouts for the same
    /// install. Matches the consistency guarantee Copilot flagged
    /// during PR #443 review.
    #[test]
    pub fn npm_config_workspace_dir_re_anchors_modules() {
        let _guard = EnvGuard::snapshot(["NPM_CONFIG_WORKSPACE_DIR", "npm_config_workspace_dir"]);

        let env_workspace = tempdir().unwrap();
        let cwd_dir = tempdir().unwrap();
        // SAFETY: lock held by `_guard`. Cleared on drop.
        //
        // Set the uppercase name only and let the lowercase name
        // keep whatever the inherited environment had. Touching the
        // lowercase name here would corrupt the test on Windows,
        // where env vars are case-insensitive: `remove_var` on
        // either spelling clears the *same* variable that
        // `set_var("NPM_CONFIG_WORKSPACE_DIR", ...)` just set, and
        // the test would observe "no env override" instead of the
        // env path. Since [`Config::current`] checks the uppercase
        // spelling first via `or_else` (matching pnpm), an
        // externally-set lowercase value is unobservable here, so
        // leaving it alone keeps both platforms green.
        unsafe {
            env::set_var("NPM_CONFIG_WORKSPACE_DIR", env_workspace.path());
        }

        let config = Config::current::<Host, _, _, _, _>(
            || cwd_dir.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("config loads");
        assert_eq!(
            config.modules_dir,
            env_workspace.path().join("node_modules"),
            "modules_dir must follow NPM_CONFIG_WORKSPACE_DIR, not the cwd",
        );
        assert_eq!(
            config.virtual_store_dir,
            env_workspace.path().join("node_modules/.pnpm"),
            "virtual_store_dir must follow NPM_CONFIG_WORKSPACE_DIR, not the cwd",
        );
    }

    /// An empty `NPM_CONFIG_WORKSPACE_DIR` falls through to the
    /// upward walk, matching pnpm's truthy `if (workspaceDir)` check.
    /// Pairs with `pacquet_workspace`'s
    /// `empty_env_var_is_treated_as_unset`.
    #[test]
    pub fn empty_npm_config_workspace_dir_falls_through() {
        let _guard = EnvGuard::snapshot(["NPM_CONFIG_WORKSPACE_DIR", "npm_config_workspace_dir"]);
        // SAFETY: lock held by `_guard`. Setting *both* names to
        // empty handles both platforms: on POSIX they're distinct
        // vars (clear each); on Windows they're aliases for the
        // same variable (the second `set_var` is a no-op). Either
        // way, both reads return empty, the truthy filter rejects
        // both, and the install falls through to the cwd-walk.
        unsafe {
            env::set_var("NPM_CONFIG_WORKSPACE_DIR", "");
            env::set_var("npm_config_workspace_dir", "");
        }
        let tmp = tempdir().unwrap();
        let config = Config::current::<Host, _, _, _, _>(
            || tmp.path().to_path_buf().pipe(Ok::<_, ()>),
            || None,
            Config::new,
        )
        .expect("config loads");
        // No yaml in tmp → no re-anchor → cwd-anchored defaults.
        assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
        assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    }
}
