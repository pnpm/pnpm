mod api;
mod defaults;
mod env_overlay;
pub mod matcher;
mod npmrc_auth;
mod store_path;
pub mod version_policy;
mod workspace_yaml;

pub use crate::api::{EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe};

use indexmap::IndexMap;
use pacquet_patching::{
    CalcPatchHashError, PatchGroupRecord, ResolvePatchedDependenciesError, calc_patch_hashes,
    resolve_and_group,
};
use pacquet_store_dir::StoreDir;
use pacquet_workspace_state::ConfigDependency;
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

pub use crate::defaults::{
    PACQUET_VERSION, available_parallelism, default_git_shallow_hosts,
    default_peers_suffix_max_length, default_unsafe_perm, default_virtual_store_dir_max_length,
    default_workspace_concurrency, is_unsafe_perm_posix, resolve_child_concurrency,
};
use crate::defaults::{
    default_cache_dir, default_child_concurrency, default_config_dir,
    default_enable_global_virtual_store, default_fetch_retries, default_fetch_retry_factor,
    default_fetch_retry_maxtimeout, default_fetch_retry_mintimeout, default_fetch_timeout,
    default_hoist_pattern, default_modules_cache_max_age, default_modules_dir,
    default_public_hoist_pattern, default_registry, default_store_dir, default_user_agent,
    default_virtual_store_dir,
};
pub use workspace_yaml::{
    GLOBAL_CONFIG_YAML_FILENAME, LoadWorkspaceYamlError, PackageExtension, PeerDependencyMeta,
    PeerDependencyRules, UpdateConfig, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings,
    workspace_root_or,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeLinker {
    /// dependencies are symlinked from a virtual store at `node_modules/.pnpm`.
    #[default]
    Isolated,

    /// flat `node_modules` without symlinks is created. Same as the `node_modules` created by npm or
    /// Yarn Classic.
    Hoisted,

    /// no `node_modules`. Plug'n'Play is an innovative strategy for Node that is used by
    /// Yarn Berry. It is recommended to also set symlink setting to false when using pnp as
    /// your linker.
    Pnp,
}

/// Controls how far dependencies are hoisted under
/// `nodeLinker: hoisted`, mirroring yarn's `nmHoistingLimits`.
///
/// Given workspace package `A` → `B` → `C`:
/// - [`HoistingLimits::None`] (default): hoist as far as possible
///   (`/node_modules/B`, `/node_modules/C`).
/// - [`HoistingLimits::Workspaces`]: hoist only as far as each
///   workspace package (`/packages/A/node_modules/{B,C}`).
/// - [`HoistingLimits::Dependencies`]: hoist only up to each
///   workspace package's direct dependencies
///   (`/packages/A/node_modules/B/node_modules/C`).
///
/// Mirrors pnpm's
/// [`HoistingLimits`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L10).
/// No effect under `nodeLinker: isolated`. The user-facing mode is
/// translated into the per-locator border map the hoister consumes
/// by `crate::get_hoisting_limits` in `pacquet-package-manager`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HoistingLimits {
    #[default]
    None,
    Workspaces,
    Dependencies,
}

/// Supply-chain trust policy applied to lockfile entries.
///
/// Mirrors pnpm's
/// [`TrustPolicy`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/core/types/src/config.ts#L5)
/// (`'no-downgrade' | 'off'`) and drives the
/// `pacquet-resolving-npm-resolver` verifier: under
/// [`TrustPolicy::NoDowngrade`] the verifier rejects any version
/// whose trust evidence (`_npmUser.trustedPublisher` or
/// `dist.attestations.provenance`) is weaker than an earlier-published
/// version's. Defaults to [`TrustPolicy::Off`] so installs without an
/// explicit policy don't change behavior.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustPolicy {
    #[default]
    Off,
    NoDowngrade,
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

impl serde::Serialize for ScriptsPrependNodePath {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self {
            ScriptsPrependNodePath::Always => serializer.serialize_bool(true),
            ScriptsPrependNodePath::Never => serializer.serialize_bool(false),
            ScriptsPrependNodePath::WarnOnly => serializer.serialize_str("warn-only"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for ScriptsPrependNodePath {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl Visitor<'_> for V {
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

/// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Tri-state: a
/// bare-semver dependency on a workspace package may resolve to the
/// local copy, or to a registry copy with the same name, or be
/// matched only when the user explicitly opts in with a `workspace:`
/// prefix.
///
/// Mirrors pnpm's `linkWorkspacePackages: boolean | 'deep'` at
/// [`Config.linkWorkspacePackages`](https://github.com/pnpm/pnpm/blob/5353fcbf01/config/reader/src/Config.ts#L189).
/// Default is [`LinkWorkspacePackages::Off`], matching pnpm's
/// [`'link-workspace-packages': false`](https://github.com/pnpm/pnpm/blob/5353fcbf01/config/reader/src/index.ts#L174)
/// fallback.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LinkWorkspacePackages {
    /// `false`. Workspace packages are matched only when the user
    /// writes a `workspace:`-prefixed range. A bare-semver range
    /// always goes to the registry.
    #[default]
    Off,
    /// `true`. Direct dependencies match workspace packages by name
    /// and version, like a `workspace:` range would; transitive
    /// dependencies still go to the registry.
    DirectOnly,
    /// `"deep"`. Both direct and transitive dependencies match
    /// workspace packages.
    Deep,
}

impl LinkWorkspacePackages {
    /// Whether the npm resolver should consult the workspace map
    /// when resolving a bare-semver wanted dependency. Mirrors pnpm's
    /// [`linkWorkspacePackagesDepth >= currentDepth`](https://github.com/pnpm/pnpm/blob/5353fcbf01/installing/deps-resolver/src/resolveDependencies.ts#L1339)
    /// gate, collapsed onto pacquet's current shape where the deps
    /// resolver passes the same `ResolveOptions` to every depth — the
    /// [`Self::DirectOnly`] arm only fires at the importer level
    /// (`current_depth == 0`); pacquet's caller decides which arm
    /// to expose by passing in the current depth.
    #[must_use]
    pub fn enabled_at_depth(self, current_depth: u32) -> bool {
        match self {
            LinkWorkspacePackages::Off => false,
            LinkWorkspacePackages::DirectOnly => current_depth == 0,
            LinkWorkspacePackages::Deep => true,
        }
    }
}

impl serde::Serialize for LinkWorkspacePackages {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self {
            LinkWorkspacePackages::Off => serializer.serialize_bool(false),
            LinkWorkspacePackages::DirectOnly => serializer.serialize_bool(true),
            LinkWorkspacePackages::Deep => serializer.serialize_str("deep"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for LinkWorkspacePackages {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl Visitor<'_> for V {
            type Value = LinkWorkspacePackages;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a boolean or the string "deep""#)
            }
            fn visit_bool<DeError: de::Error>(self, value: bool) -> Result<Self::Value, DeError> {
                Ok(if value {
                    LinkWorkspacePackages::DirectOnly
                } else {
                    LinkWorkspacePackages::Off
                })
            }
            fn visit_str<DeError: de::Error>(self, value: &str) -> Result<Self::Value, DeError> {
                match value {
                    "deep" => Ok(LinkWorkspacePackages::Deep),
                    other => Err(DeError::invalid_value(
                        de::Unexpected::Str(other),
                        &r#"true, false, or "deep""#,
                    )),
                }
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// How the resolver picks a version for a direct dependency when more
/// than one satisfies the wanted range.
///
/// Mirrors pnpm's
/// [`resolutionMode`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/Config.ts#L135)
/// (`'highest' | 'time-based' | 'lowest-direct'`). Defaults to
/// [`ResolutionMode::Highest`], matching pnpm's
/// [`'resolution-mode': 'highest'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L188).
///
/// Only direct dependencies are affected by the lowest-version pick;
/// subdependencies are always picked highest. Under
/// [`ResolutionMode::TimeBased`] the resolver additionally constrains
/// subdependencies to versions published no later than the newest
/// resolved direct dependency (plus a one-hour delta).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionMode {
    /// Pick the highest version that satisfies the range, everywhere.
    #[default]
    Highest,

    /// Resolve direct dependencies to their lowest satisfying version,
    /// then resolve subdependencies from versions published before the
    /// last direct dependency was published.
    TimeBased,

    /// Resolve direct dependencies to their lowest satisfying version;
    /// subdependencies are unconstrained (picked highest).
    LowestDirect,
}

impl ResolutionMode {
    /// Whether direct dependencies are resolved to their lowest
    /// satisfying version. True for both [`Self::TimeBased`] and
    /// [`Self::LowestDirect`]. Mirrors pnpm's
    /// [`pickLowestVersion`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-resolver/src/resolveDependencies.ts#L470)
    /// computation.
    #[must_use]
    pub fn picks_lowest_direct(self) -> bool {
        matches!(self, ResolutionMode::TimeBased | ResolutionMode::LowestDirect)
    }
}

/// How `pnpm add` / `pnpm update` reconcile a directly-specified version
/// against a `catalog:` entry for the same package. Mirrors pnpm's
/// [`catalogMode`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L186)
/// setting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogMode {
    /// The catalog is consulted only for explicit `catalog:` specifiers;
    /// `add` / `update` never reconcile a direct version against it. The
    /// default, matching pnpm's
    /// [`'catalog-mode': 'manual'`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L132).
    #[default]
    Manual,

    /// A direct version that disagrees with the matching catalog entry is
    /// an error (`ERR_PNPM_CATALOG_VERSION_MISMATCH`).
    Strict,

    /// A direct version that disagrees with the matching catalog entry is
    /// kept, with a warning; a version that agrees is used from the
    /// catalog.
    Prefer,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// When true, all dependencies are hoisted to `node_modules/.pnpm/node_modules`.
    /// This makes unlisted dependencies accessible to all packages inside `node_modules`.
    #[default = true]
    pub hoist: bool,

    /// Tells pnpm which packages should be hoisted to `node_modules/.pnpm/node_modules`.
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
    /// Default is `Some([])`, mirroring pnpm v11's
    /// [`'public-hoist-pattern': []`](https://github.com/pnpm/pnpm/blob/1627943d2a/config/reader/src/index.ts#L184)
    /// — any non-empty default would write a `publicHoistPattern`
    /// into `.modules.yaml` that the next `pnpm` invocation rejects
    /// with `ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF`
    /// ([pnpm/pnpm#11750](https://github.com/pnpm/pnpm/issues/11750)).
    #[default(_code = "Some(default_public_hoist_pattern())")]
    pub public_hoist_pattern: Option<Vec<String>>,

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

    /// The directory in which dependencies will be installed (instead of `node_modules`).
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
    /// defaults to 120 to leave headroom for `node_modules/<name>`
    /// suffixes appended below).
    ///
    /// Configurable via `virtualStoreDirMaxLength` in
    /// `pnpm-workspace.yaml`, global `config.yaml`, or
    /// `PNPM_CONFIG_VIRTUAL_STORE_DIR_MAX_LENGTH`. Mirrors upstream
    /// `Config.virtualStoreDirMaxLength` at
    /// <https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/Config.ts>.
    /// The same value is persisted into `node_modules/.modules.yaml`
    /// so subsequent installs see the user's pick.
    ///
    /// Default value is 120.
    #[default(_code = "default_virtual_store_dir_max_length()")]
    pub virtual_store_dir_max_length: u64,

    /// Cap on the rendered peer-suffix length before the suffix is
    /// replaced with a short hash. Mirrors upstream
    /// `Config.peersSuffixMaxLength` and is threaded into
    /// `pacquet_deps_path::create_peer_dep_graph_hash` — when the
    /// flattened `(peer@ver)(peer@ver)…` string exceeds this many
    /// bytes, pnpm and pacquet swap it for a 32-char sha256 hash so
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
    /// lockfile by default — matching upstream pnpm's
    /// [`useLockfile`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/src/install/extendInstallOptions.ts#L323)
    /// default.
    #[default = true]
    pub lockfile: bool,

    /// When set to true and the available pnpm-lock.yaml satisfies the package.json dependencies
    /// directive, a headless installation is performed. A headless installation skips all
    /// dependency resolution as it does not need to modify the lockfile.
    #[default = true]
    pub prefer_frozen_lockfile: bool,

    /// When `true`, `pacquet install` performs a workspace-state
    /// freshness check before any of the install setup runs and
    /// returns immediately ("Already up to date") if nothing has
    /// changed since the previous install.
    ///
    /// Mirrors pnpm's `optimisticRepeatInstall` setting and the
    /// [`checkDepsStatus`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts)
    /// dispatch in [`installDeps`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/commands/src/installDeps.ts#L179-L194).
    /// The fast path keys off `.pnpm-workspace-state-v1.json`'s
    /// `lastValidatedTimestamp` vs each project's `package.json`
    /// mtime, so it never reads the lockfile or the verifier cache
    /// when no manifest has been touched.
    ///
    /// Defaults to `true` to match upstream
    /// ([`config/reader/src/index.ts:169`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/config/reader/src/index.ts#L169)).
    #[default = true]
    pub optimistic_repeat_install: bool,

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
    /// prefetch and the `SQLite` `index.db` lookup miss, the tarball
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

    /// User-defined named-registry aliases from
    /// `pnpm-workspace.yaml#namedRegistries`. Maps each alias name
    /// (`gh`, `work`, ...) to the registry URL its `<alias>:` specifiers
    /// resolve against. Empty by default — the resolver layer merges
    /// these on top of pnpm's built-in defaults (today: `gh:` →
    /// GitHub Packages) and rejects malformed URLs at construction
    /// time with `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    ///
    /// Mirrors upstream's
    /// [`namedRegistries`](https://github.com/pnpm/pnpm/blob/b61e268d57/config/reader/src/Config.ts#L227)
    /// setting.
    pub named_registries: BTreeMap<String, String>,

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

    /// When `true`, dependencies declared with the `link:` protocol
    /// are excluded from `pnpm-lock.yaml`. Workspace-protocol
    /// dependencies (`workspace:`), which also resolve to a link,
    /// are still recorded. Mirrors pnpm's
    /// [`excludeLinksFromLockfile`](https://github.com/pnpm/pnpm/blob/094aa6e57b/config/reader/src/Config.ts#L71)
    /// (default `false` per
    /// [`config/reader/src/index.ts`](https://github.com/pnpm/pnpm/blob/094aa6e57b/config/reader/src/index.ts#L144)).
    pub exclude_links_from_lockfile: bool,

    /// When `true`, conflicting peer-dependency ranges from multiple
    /// consumers are merged with `||` (so the resolver may pick the
    /// highest version that satisfies any one of them) instead of
    /// being dropped when their intersection is empty. Mirrors pnpm's
    /// [`autoInstallPeersFromHighestMatch`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L796-L818).
    pub auto_install_peers_from_highest_match: bool,

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
    /// `hoistingLimits` from `pnpm-workspace.yaml`. Controls how far
    /// dependencies are hoisted under `nodeLinker: hoisted`. See
    /// [`HoistingLimits`] for the `none` / `workspaces` /
    /// `dependencies` semantics. Default [`HoistingLimits::None`]
    /// (hoist as far as possible). Translated into the hoister's
    /// per-locator border map by `crate::get_hoisting_limits` in
    /// `pacquet-package-manager`. No effect under
    /// `nodeLinker: isolated`.
    pub hoisting_limits: HoistingLimits,

    /// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Controls
    /// whether the npm resolver consults the workspace map when
    /// resolving bare-semver wanted dependencies. See
    /// [`LinkWorkspacePackages`] for the tri-state semantics.
    /// Default `false`, matching pnpm's
    /// [`'link-workspace-packages': false`](https://github.com/pnpm/pnpm/blob/5353fcbf01/config/reader/src/index.ts#L174).
    pub link_workspace_packages: LinkWorkspacePackages,

    /// `injectWorkspacePackages` from `pnpm-workspace.yaml`. When
    /// `true`, workspace-package resolutions materialize as `file:`
    /// (hard-linked copies into the virtual store) instead of `link:`
    /// symlinks back to the source. Per-dependency
    /// `dependenciesMeta[*].injected = true` opts a single dep into
    /// the same behavior even when this flag is `false`.
    ///
    /// Default `false`, matching pnpm's
    /// [`'inject-workspace-packages': undefined`](https://github.com/pnpm/pnpm/blob/39101f5e37/config/reader/src/Config.ts#L190).
    pub inject_workspace_packages: bool,

    /// When `true`, prefer a workspace package over a registry pick
    /// even when the registry version is newer than the workspace
    /// one. Mirrors pnpm's
    /// [`preferWorkspacePackages`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L191).
    /// Consumed by the npm resolver's
    /// [registry-pick + workspace shadow](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/src/index.ts#L550-L582).
    /// Default `false`, matching pnpm's
    /// [`'prefer-workspace-packages': false`](https://github.com/pnpm/pnpm/blob/a23956e3ab/config/reader/src/index.ts#L183).
    pub prefer_workspace_packages: bool,

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

    /// When `true`, peer-dependency suffixes in `depPath`s use
    /// version-only identifiers (`name@version`) instead of recursive
    /// dep paths, eliminating nested suffixes like
    /// `(foo@1.0.0(bar@2.0.0))`. Mirrors pnpm's
    /// [`dedupePeers`](https://github.com/pnpm/pnpm/blob/39101f5e37/config/reader/src/Config.ts#L218).
    /// Default `false`, matching pnpm's
    /// [`dedupe-peers`](https://github.com/pnpm/pnpm/blob/39101f5e37/config/reader/src/index.ts#L138).
    pub dedupe_peers: bool,

    /// When `true`, a direct dependency of a non-root workspace
    /// project is omitted from that project's `node_modules/` when
    /// the workspace root resolves the same alias to the same target.
    /// Drives both the linking step (which skips writing the
    /// per-importer symlink) and bin linking (the deduped dep won't
    /// reappear under the project's `node_modules/.bin`).
    ///
    /// Default `false`, matching pnpm's config-reader default at
    /// [`config/reader/src/index.ts:139`](https://github.com/pnpm/pnpm/blob/a23956e3ab/config/reader/src/index.ts#L139)
    /// (`'dedupe-direct-deps': false`). The linker call site is at
    /// [`installing/deps-restorer/src/index.ts:777`](https://github.com/pnpm/pnpm/blob/a23956e3ab/installing/deps-restorer/src/index.ts#L777).
    #[default = false]
    pub dedupe_direct_deps: bool,

    /// When `true`, injected workspace dependencies whose materialised
    /// children turn out to be a subset of the target workspace
    /// project's own direct dependencies get rewritten back to
    /// symlinks. Mirrors pnpm's
    /// [`dedupeInjectedDeps`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/dedupeInjectedDeps.ts).
    /// Default `true` to match pnpm's
    /// [`extendInstallOptions`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/extendInstallOptions.ts#L282).
    #[default = true]
    pub dedupe_injected_deps: bool,

    /// If this is enabled, commands will fail if there is a missing or invalid peer dependency in the tree.
    pub strict_peer_dependencies: bool,

    /// When enabled, dependencies of the root workspace project are used to resolve peer
    /// dependencies of any projects in the workspace. It is a useful feature as you can install
    /// your peer dependencies only in the root of the workspace, and you can be sure that all
    /// projects in the workspace use the same versions of the peer dependencies.
    #[default = true]
    pub resolve_peers_from_workspace_root: bool,

    /// When `true`, reject exotic (git, tarball, file, ...) dependencies
    /// reached transitively from the importer. Direct deps remain
    /// allowed. Mirrors pnpm's
    /// [`blockExoticSubdeps`](https://github.com/pnpm/pnpm/blob/df990fdb51/config/reader/src/Config.ts#L222).
    /// Default `true` to match pnpm v11's
    /// [`block-exotic-subdeps`](https://github.com/pnpm/pnpm/blob/df990fdb51/config/reader/src/index.ts#L187).
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

    /// Maximum number of concurrent network requests pacquet keeps
    /// in flight during install — the size of the [`pacquet_network`]
    /// semaphore. Mirrors pnpm's `networkConcurrency`; the default is
    /// pnpm's `Math.min(64, Math.max(calcMaxWorkers() * 3, 16))`
    /// formula, implemented by [`pacquet_network::default_network_concurrency`].
    #[default(_code = "pacquet_network::default_network_concurrency()")]
    pub network_concurrency: usize,

    /// Per-request network timeout in milliseconds. Mirrors pnpm's
    /// `fetchTimeout` (default `60000` — 60 s, see
    /// [`pacquet_network::DEFAULT_FETCH_TIMEOUT_MS`]). Applied as both
    /// the response and connect deadline of the reqwest client.
    #[default(_code = "default_fetch_timeout()")]
    pub fetch_timeout: u64,

    /// Value of the `User-Agent` header sent on every registry request.
    /// Mirrors pnpm's `userAgent`; the default is pnpm's
    /// `pnpm/pacquet-<version> npm/? node/? <platform> <arch>` format (built by
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

    /// Path to the user-level `.npmrc` to read auth from, overriding the
    /// default `~/.npmrc`. Mirrors pnpm's `npmrcAuthFile` (and the
    /// `--userconfig` alias). Resolved in [`Config::current`] from this
    /// field (set by the CLI flag) then the `PNPM_CONFIG_NPMRC_AUTH_FILE`
    /// / `PNPM_CONFIG_USERCONFIG` / `npm_config_userconfig` env vars.
    /// `None` falls back to `~/.npmrc`.
    pub npmrc_auth_file: Option<PathBuf>,

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

    /// `enablePrePostScripts` from `pnpm-workspace.yaml`. When `true`,
    /// `pnpm run <name>` also runs the `pre<name>` and `post<name>`
    /// scripts if they exist. Defaults to `true`, matching pnpm's
    /// [`defaultOptions['enable-pre-post-scripts']`](https://github.com/pnpm/pnpm/blob/a23956e3ab/config/reader/src/index.ts#L143).
    #[default = true]
    pub enable_pre_post_scripts: bool,

    /// `scriptShell` from `pnpm-workspace.yaml`. The shell used to run
    /// scripts and `pnpm exec`. `None` selects the platform default
    /// (`sh` on POSIX, `cmd.exe` on Windows). Mirrors pnpm's
    /// [`Config.scriptShell`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L95).
    pub script_shell: Option<String>,

    /// `nodeOptions` from `pnpm-workspace.yaml`. When set, it is exported
    /// as `NODE_OPTIONS` to scripts and `pnpm exec` child processes.
    /// Mirrors pnpm's
    /// [`Config.nodeOptions`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L251).
    pub node_options: Option<String>,

    /// `extraBinPaths`: directories prepended to `PATH` (after the
    /// project's own `node_modules/.bin`) when running scripts and
    /// `pnpm exec`. pnpm computes this as the workspace root's
    /// `node_modules/.bin` inside a workspace and leaves it empty
    /// otherwise. pacquet defaults it empty until workspace support
    /// lands. Mirrors pnpm's
    /// [`Config.extraBinPaths`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L72).
    pub extra_bin_paths: Vec<PathBuf>,

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

    /// `workspaceConcurrency` from `pnpm-workspace.yaml` / global
    /// `config.yaml` / `PNPM_CONFIG_WORKSPACE_CONCURRENCY`, overridable
    /// per-invocation by the `--workspace-concurrency` CLI flag. The
    /// maximum number of workspace projects pnpm processes in parallel
    /// during a recursive operation. Resolved through
    /// [`resolve_child_concurrency`] (upstream's
    /// [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34))
    /// so a non-positive yaml/CLI value is read as
    /// `parallelism - |value|` (floored at 1).
    ///
    /// Default: `min(4, availableParallelism())`, matching upstream's
    /// [`getDefaultWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L21-L23)
    /// default at
    /// [`config/reader/src/index.ts:208`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L208).
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
    /// current directory. Mirrors pnpm's CLI-only
    /// [`recursive`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/Config.ts#L130)
    /// boolean: it is not a `.npmrc` / `pnpm-workspace.yaml` key, so
    /// the yaml / env overlay never populates it — the CLI layer sets
    /// it from the flag.
    ///
    /// pacquet's install already spans the whole workspace (it reads
    /// every importer from the shared lockfile), so the flag is a
    /// surface no-op on `install` today. Stored for parity and for
    /// future commands where recursive vs. single-project selection
    /// diverges.
    pub recursive: bool,

    /// `--filter` selectors, one raw selector string per entry
    /// (`@scope/*`, `./pkg`, `foo...`, `!bar`, ...), parsed by
    /// `pacquet-workspace-projects-filter`. Mirrors pnpm's CLI-only
    /// [`filter`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L75)
    /// array: not a `.npmrc` / `pnpm-workspace.yaml` key, so only the
    /// CLI layer populates it.
    pub filter: Vec<String>,

    /// `--filter-prod` selectors. Same shape as [`Self::filter`], but
    /// each selector follows production dependencies only when its
    /// dependency walk runs. Mirrors pnpm's CLI-only
    /// [`filterProd`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L76)
    /// array.
    pub filter_prod: Vec<String>,

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

    /// `overrides` from `pnpm-workspace.yaml`. Raw `selector → spec`
    /// map; see [`WorkspaceSettings::overrides`] for the field's
    /// contract. `$dep-name` self-references are resolved against
    /// the root manifest's direct deps before this field lands here.
    /// Empty maps collapse to `None`. Drives the read-package hook
    /// that rewrites manifests during install, and the lockfile-side
    /// drift check at
    /// [`getOutdatedLockfileSetting.ts:50-52`](https://github.com/pnpm/pnpm/blob/606f53e78f/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L50-L52).
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
    /// [`PackageExtension`] for the entry shape.
    ///
    /// [`WorkspaceSettings::package_extensions`]: crate::workspace_yaml::WorkspaceSettings::package_extensions
    pub package_extensions: Option<IndexMap<String, workspace_yaml::PackageExtension>>,

    /// pnpm's packument cache directory. Used by the lockfile
    /// verification gate to memoize past results in
    /// `<cache_dir>/lockfile-verified.jsonl`, and by the npm verifier
    /// to mirror full-metadata responses for conditional GETs.
    /// Share a writable cache only between mutually trusted users,
    /// jobs, and processes.
    ///
    /// Mirrors pnpm's
    /// [`cacheDir`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L159);
    /// the default resolution chain ports
    /// [`getCacheDir`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/dirs.ts#L4-L23).
    #[default(_code = "default_cache_dir::<Host>()")]
    pub cache_dir: PathBuf,

    /// `dlxCacheMaxAge`: the maximum age in **minutes** of a cached
    /// `pnpm dlx` install before it is rebuilt from scratch. Defaults to
    /// `1440` (24 hours). Mirrors pnpm's
    /// [`Config.dlxCacheMaxAge`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L211).
    #[default(_code = "24 * 60")]
    pub dlx_cache_max_age: u64,

    /// Minimum age, in **minutes**, a published version must reach
    /// before pacquet accepts it. Drives the
    /// `MINIMUM_RELEASE_AGE_VIOLATION` verifier check on every
    /// `(name, version)` entry the lockfile loads under this policy.
    /// `None` disables the check entirely.
    ///
    /// Default: `Some(1440)` (24 hours), matching upstream pnpm's
    /// built-in at
    /// [`config/reader/src/index.ts:176`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L176).
    /// Mirrors pnpm's
    /// [`minimumReleaseAge`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L264)
    /// in minutes — the same unit pnpm's CLI / yaml accept and pnpm
    /// forwards verbatim through `extendInstallOptions` to the
    /// verifier.
    #[default(_code = "Some(24 * 60)")]
    pub minimum_release_age: Option<u64>,

    /// Glob-style `name[@version]` patterns that opt specific packages
    /// out of the [`minimum_release_age`] check. Empty / `None` means
    /// no exclusions. Mirrors pnpm's
    /// [`minimumReleaseAgeExclude`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L265).
    ///
    /// [`minimum_release_age`]: Self::minimum_release_age
    pub minimum_release_age_exclude: Option<Vec<String>>,

    /// When the registry's metadata lacks the per-version `time`
    /// field (some self-hosted registries strip it), the verifier
    /// cannot enforce the maturity cutoff. With this flag set,
    /// uncheckable entries pass with a one-time `globalWarn` instead
    /// of failing closed. Mirrors pnpm's
    /// [`minimumReleaseAgeIgnoreMissingTime`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L266),
    /// which defaults to `true` so a registry that strips `time`
    /// (a self-hosted Verdaccio without provenance plugin, for
    /// example) doesn't lock the user out.
    #[default = true]
    pub minimum_release_age_ignore_missing_time: bool,

    /// When `true`, picks fresher-than-cutoff versions still abort
    /// rather than auto-collect into [`Self::minimum_release_age_exclude`].
    /// Used by the resolver path; the verifier itself does not gate
    /// on this flag. Mirrors pnpm's
    /// [`minimumReleaseAgeStrict`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L267).
    ///
    /// Upstream conditional default: `true` when
    /// `minimumReleaseAge` is explicitly configured, `false`
    /// otherwise. Modeled as [`Option`] here so the deserializer can
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
    /// weaker policy than CI enforces) will slip through. Mirrors
    /// pnpm's [`trustLockfile`](https://github.com/pnpm/pnpm/blob/main/config/reader/src/Config.ts).
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

    /// Glob-style `name[@version]` patterns that opt specific packages
    /// out of the [`trust_policy`] check. Mirrors pnpm's
    /// [`trustPolicyExclude`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L271).
    ///
    /// [`trust_policy`]: Self::trust_policy
    pub trust_policy_exclude: Option<Vec<String>>,

    /// Cutoff in minutes after which the trust check skips a
    /// version that's old enough — once a package has been published
    /// for long enough, the supply-chain assumption is that any
    /// downgrade would have already surfaced. Mirrors pnpm's
    /// [`trustPolicyIgnoreAfter`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L272).
    pub trust_policy_ignore_after: Option<u64>,

    /// How direct dependencies pick a version when several satisfy the
    /// wanted range, and whether subdependencies are constrained by
    /// publication date. See [`ResolutionMode`]. Default
    /// [`ResolutionMode::Highest`], matching pnpm's
    /// [`'resolution-mode': 'highest'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L188).
    pub resolution_mode: ResolutionMode,

    /// How `pnpm add` / `pnpm update` reconcile a directly-specified
    /// version against a matching `catalog:` entry. See [`CatalogMode`].
    /// Default [`CatalogMode::Manual`], matching pnpm's
    /// [`'catalog-mode': 'manual'`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L132).
    pub catalog_mode: CatalogMode,

    /// Catalogs injected by an `updateConfig` pnpmfile hook, seeded from
    /// `pnpm-workspace.yaml`'s `catalog:`/`catalogs:` and returned
    /// (possibly modified) by the hook. `None` when no hook changed
    /// them, in which case the install reads catalogs straight from the
    /// workspace manifest. `Some` carries the complete catalog set the
    /// hook produced (existing + injected), so the install uses it as-is
    /// — the counterpart to pnpm's `config.catalogs` after the
    /// `updateConfig` pass.
    pub catalogs: Option<pacquet_catalogs_types::Catalogs>,

    /// Name of the catalog `pnpm add` saves a new dependency into,
    /// set by `--save-catalog-name=<name>` (with `--save-catalog` a
    /// shorthand for `default`). When `Some`, an `add` writes
    /// `catalog:`/`catalog:<name>` to the manifest and inserts the
    /// entry into `pnpm-workspace.yaml` even under
    /// [`CatalogMode::Manual`]. Mirrors pnpm's
    /// [`saveCatalogName`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/Config.ts#L92)
    /// (default
    /// [`undefined`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L191)).
    /// A CLI-only flag in pnpm
    /// ([`excludedPnpmKeys`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/configFileKey.ts#L138)),
    /// so pacquet does not read it from `pnpm-workspace.yaml`; the
    /// effective value is threaded onto the `add` command from the CLI.
    pub save_catalog_name: Option<String>,

    /// Whether the configured registry returns the per-version `time`
    /// field in its *abbreviated* metadata. When `false` (the default),
    /// [`ResolutionMode::TimeBased`] resolution (and the
    /// [`TrustPolicy::NoDowngrade`] check) must fetch full metadata to
    /// obtain publication dates. Setting this to `true` for a registry
    /// that includes `time` in abbreviated metadata (Verdaccio 5.15.1+)
    /// avoids the slower full-metadata fetch. Mirrors pnpm's
    /// [`registrySupportsTimeField`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/Config.ts#L136)
    /// (default
    /// [`false`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L212)).
    pub registry_supports_time_field: bool,

    /// `name → semver-range` map of deprecated package versions whose
    /// deprecation warning should be suppressed. A deprecated package
    /// is reported unless its name has an entry here whose range the
    /// resolved version satisfies. Mirrors pnpm's
    /// [`allowedDeprecatedVersions`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L153)
    /// and its consumer at
    /// [`resolveDependencies.ts:1593-1606`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolveDependencies.ts#L1593-L1606).
    ///
    /// Parsed and stored for parity with pnpm's config surface. Pacquet
    /// does not yet emit deprecation warnings during resolution, so
    /// there is nothing for the allow-list to suppress today; the field
    /// is consumed once that warning path lands.
    pub allowed_deprecated_versions: BTreeMap<String, String>,

    /// `updateConfig` from `pnpm-workspace.yaml`. Today only its
    /// `ignoreDependencies` field is modeled — the list of dependency
    /// name patterns `pnpm update` skips. Mirrors pnpm's
    /// [`updateConfig`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L193-L195)
    /// and its consumer in
    /// [`installing/commands/src/update`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/commands/src/update/index.ts#L212).
    ///
    /// Parsed and stored for parity with pnpm's config surface. Pacquet
    /// has no `update` / `outdated` command yet, so the ignore list has
    /// no consumer today; it is honored once that command lands.
    pub update_config: workspace_yaml::UpdateConfig,

    /// `peerDependencyRules` from `pnpm-workspace.yaml`: customizations
    /// applied when reporting peer-dependency issues. See
    /// [`PeerDependencyRules`]. Mirrors pnpm's
    /// [`peerDependencyRules`](https://github.com/pnpm/pnpm/blob/39101f5e37/core/types/src/package.ts#L147-L151)
    /// and its consumer
    /// [`reportPeerDependencyIssues`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/reportPeerDependencyIssues.ts).
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
    /// fetchers via [`pacquet_network::AuthHeaders::for_url`]. Empty
    /// when no `.npmrc` was found or no auth keys were set.
    pub auth_headers: std::sync::Arc<pacquet_network::AuthHeaders>,
}

impl Config {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Effective value of [`Self::minimum_release_age_strict`].
    /// Returns the user-supplied value when set, else `false`.
    ///
    /// Upstream pnpm flips this to `true` when the user *explicitly*
    /// set `minimumReleaseAge` (see
    /// [`config/reader/src/index.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts)'s
    /// post-parse hook), but the "explicitly set vs default" check
    /// relies on the `explicitlySetKeys` tracker pnpm's reader
    /// maintains, which pacquet's config layer doesn't have yet.
    /// Without that, distinguishing the built-in 1440-minute default
    /// from a user-typed `minimumReleaseAge: 1440` isn't possible,
    /// so this resolver stays conservative: explicit `true` /
    /// `false` from yaml wins, otherwise `false`. The verifier
    /// itself doesn't gate on this flag — it's resolver-only — so
    /// the conservative default is dormant until pacquet ports the
    /// resolver and the `explicitlySetKeys` mechanism alongside it.
    pub fn resolved_minimum_release_age_strict(&self) -> bool {
        self.minimum_release_age_strict.unwrap_or(false)
    }

    /// Effective [`Self::minimum_release_age`], with `Some(0)` treated
    /// as "disabled" (`None`).
    ///
    /// Mirrors pnpm's falsy check
    /// ([`opts.minimumReleaseAge ? ... : undefined`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L48-L57)):
    /// `minimumReleaseAge: 0` disables the maturity cutoff. Disabling it
    /// is also what makes `resolutionMode: lowest-direct` / `time-based`
    /// observable for direct dependencies — while a cutoff is active the
    /// picker always prefers the highest mature version, overriding the
    /// lowest-version pick (matching pnpm's
    /// [`pickRespectingMinReleaseAge`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/resolving/npm-resolver/src/pickPackage.ts#L119-L132)).
    pub fn resolved_minimum_release_age(&self) -> Option<u64> {
        self.minimum_release_age.filter(|&minutes| minutes > 0)
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
    /// `InstallWithFreshLockfile` path that upstream pnpm doesn't have.
    /// Mutating `virtual_store_dir` would redirect that path to
    /// `<storeDir>/links` too — but the issue (pnpm/pacquet#432)
    /// scopes GVS to frozen-lockfile installs. Splitting the field
    /// keeps the fresh-lockfile path on the project-local layout
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

    /// Return the `virtualStoreDir` value pnpm exposes externally — the
    /// path written into `.modules.yaml` and emitted in the `pnpm:context`
    /// NDJSON event.
    ///
    /// Upstream pnpm mutates `virtualStoreDir` in place inside
    /// [`extendInstallOptions.ts:419-422`](https://github.com/pnpm/pnpm/blob/f2a4d2caef/installing/deps-installer/src/install/extendInstallOptions.ts#L419-L422)
    /// when `enableGlobalVirtualStore` is on and the user hasn't pinned
    /// `virtualStoreDir`, so every consumer that reads `ctx.virtualStoreDir`
    /// — including [`writeModulesManifest`](https://github.com/pnpm/pnpm/blob/f2a4d2caef/installing/modules-yaml/src/index.ts#L111-L138)
    /// and the [`pnpm:context` debug log](https://github.com/pnpm/pnpm/blob/f2a4d2caef/installing/context/src/index.ts#L196-L201)
    /// — sees the GVS-derived path.
    ///
    /// Pacquet deliberately keeps [`Self::virtual_store_dir`] at its
    /// project-local value (see [`Self::apply_global_virtual_store_derivation`]
    /// for the why), so consumers that need the externally-observable
    /// value must route through this helper instead of reading the field
    /// directly. Otherwise the `.modules.yaml` round-trip mismatches
    /// pnpm's, and the next `pnpm install` trips
    /// `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE_DIR` → forces a
    /// "modules directories will be reinstalled from scratch" prompt
    /// on every install.
    pub fn effective_virtual_store_dir(&self) -> &Path {
        if self.enable_global_virtual_store {
            &self.global_virtual_store_dir
        } else {
            &self.virtual_store_dir
        }
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

    /// Resolve relative patch file paths in
    /// [`Config::patched_dependencies`] against
    /// [`Config::workspace_dir`] and hash each file, producing the
    /// `patchedDependencies` map the lockfile records: each configured
    /// key mapped to its patch file's SHA-256 hex digest.
    ///
    /// Mirrors upstream's
    /// [`calcPatchHashes(opts.patchedDependencies)`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L547-L549),
    /// where `opts.patchedDependencies` is the manifest-dir-resolved
    /// map produced by
    /// [`getOptionsFromRootManifest`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/getOptionsFromRootManifest.ts#L44-L46).
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
        let (Some(workspace_dir), Some(raw)) = (&self.workspace_dir, &self.patched_dependencies)
        else {
            return Ok(None);
        };
        let resolved = raw.iter().map(|(key, rel_or_abs)| {
            let candidate = Path::new(rel_or_abs);
            let path = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                workspace_dir.join(candidate)
            };
            (key.clone(), path)
        });
        Ok(Some(calc_patch_hashes(resolved)?))
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
    pub fn current<Sys>(
        mut self,
        start_dir: &std::path::Path,
    ) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetHomeDir + LinkProbe,
    {
        // Re-anchor the path-valued defaults (`modules_dir`,
        // `virtual_store_dir`) onto the caller-supplied starting directory.
        // SmartDefault populates them via [`defaults::default_modules_dir`] /
        // [`defaults::default_virtual_store_dir`], which both anchor at
        // `env::current_dir()`. That diverges from `start_dir` whenever the
        // caller passed a different directory (notably
        // `pacquet --dir <path>` from elsewhere), so without this fixup
        // pacquet would load config from `<path>` while still installing
        // to the process-cwd `node_modules`. Matches pnpm 11, whose
        // `modulesDir`/`virtualStoreDir` defaults are resolved against
        // `pnpmConfig.dir`.
        self.modules_dir = start_dir.join("node_modules");
        self.virtual_store_dir = start_dir.join("node_modules/.pnpm");

        // Read the project/workspace .npmrc plus trusted user-level sources
        // and apply only the auth/network subset. Everything else is
        // intentionally ignored.
        //
        // pnpm reads several `.npmrc` sources and merges them
        // (`user < auth.ini < workspace`), pinning each file's *unscoped*
        // credentials to that file's own registry *before* the merge so
        // a higher-priority file (or `pnpm-workspace.yaml`) can never
        // pull them to a different host. See
        // [`NpmrcAuth::rescope_unscoped`] and pnpm's
        // [`loadNpmrcConfig`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/loadNpmrcFiles.ts).
        //
        // The global `config.yaml` is loaded up front: its `npmrcAuthFile`
        // participates in the user-level path resolution below, and its
        // directory is where `auth.ini` lives.
        let global_config_dir = default_config_dir::<Sys>();
        let mut global_settings =
            global_config_dir.as_deref().map(WorkspaceSettings::load_global).transpose()?.flatten();
        if let Some(global_settings) = global_settings.as_mut() {
            global_settings.substitute_env_trusted::<Sys>();
        }

        // Resolve the workspace dir before reading the project `.npmrc`
        // so subdirectory invocations use the workspace-root config,
        // matching pnpm's `opts.workspaceDir ?? localPrefix` boundary.
        let env_workspace_dir = Sys::var_os("NPM_CONFIG_WORKSPACE_DIR")
            .or_else(|| Sys::var_os("npm_config_workspace_dir"))
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
        } else {
            WorkspaceSettings::find_and_load(start_dir)?.map(|(path, settings)| {
                let base_dir = path.parent().unwrap_or(start_dir).to_path_buf();
                (base_dir, Some(settings))
            })
        };

        // Resolve the user-level `.npmrc` path. Precedence (pnpm's
        // [`index.ts:230`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/index.ts#L230)):
        // the `npmrc_auth_file` field (CLI `--npmrc-auth-file` /
        // `--userconfig`) > `PNPM_CONFIG_NPMRC_AUTH_FILE` >
        // `PNPM_CONFIG_USERCONFIG` > global `config.yaml`'s `npmrcAuthFile`
        // > `npm_config_userconfig`. Each env var is empty-filtered
        // individually (pnpm's `value !== ''`).
        let user_npmrc_path = self.npmrc_auth_file.clone().or_else(|| {
            read_pnpm_env::<Sys>("npmrc_auth_file", "NPMRC_AUTH_FILE")
                .or_else(|| read_pnpm_env::<Sys>("userconfig", "USERCONFIG"))
                .map(PathBuf::from)
                .or_else(|| {
                    global_settings
                        .as_ref()
                        .and_then(|settings| settings.npmrc_auth_file.clone())
                        .map(PathBuf::from)
                })
                .or_else(|| read_npm_env::<Sys>("userconfig", "USERCONFIG").map(PathBuf::from))
        });

        // Build the merge sources in priority order (high → low):
        // project `.npmrc` > `auth.ini` > user-level `.npmrc`. Each is
        // parsed and rescoped independently before being folded together.
        let parse_trusted_source = |text: String, dir: PathBuf, label: &str| {
            let mut auth = crate::npmrc_auth::NpmrcAuth::from_ini::<Sys>(&text, &dir);
            auth.rescope_unscoped(label);
            auth
        };
        let project_npmrc_dir =
            workspace_yaml.as_ref().map_or(start_dir, |(base_dir, _)| base_dir.as_path());
        let project_source = read_npmrc(project_npmrc_dir).map(|text| {
            let mut auth =
                crate::npmrc_auth::NpmrcAuth::from_project_ini::<Sys>(&text, project_npmrc_dir);
            auth.rescope_unscoped("<project>/.npmrc");
            auth
        });
        let auth_ini_source = global_config_dir.as_deref().and_then(|dir| {
            read_npmrc_file(&dir.join("auth.ini"))
                .map(|text| parse_trusted_source(text, dir.to_path_buf(), "auth.ini"))
        });
        let user_source = match &user_npmrc_path {
            Some(path) => read_npmrc_file(path).map(|text| {
                // Relative `cafile`/`certfile` entries resolve against
                // the file's directory; for a bare filename (no parent)
                // that's the empty path — i.e. the process cwd — never
                // the file itself.
                let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                parse_trusted_source(text, dir, "<user>/.npmrc")
            }),
            None => Sys::home_dir().and_then(|dir| {
                read_npmrc(&dir).map(|text| parse_trusted_source(text, dir, "~/.npmrc"))
            }),
        };

        // Fold high-priority-first: the first present source is the
        // base, each lower source fills the gaps it left
        // ([`NpmrcAuth::merge_under`]).
        let mut sources = [project_source, auth_ini_source, user_source].into_iter().flatten();
        let mut npmrc_auth = sources.next().unwrap_or_default();
        for lower in sources {
            npmrc_auth.merge_under(lower);
        }

        npmrc_auth.apply_registry_and_warn(&mut self);
        // Proxy cascade fires unconditionally — even when no `.npmrc`
        // is found — because the env-var fallback in pnpm's
        // [`config/reader/src/index.ts:591-600`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L591-L600)
        // is a normalization step on the resolved config, not a
        // function of `.npmrc` presence.
        npmrc_auth.apply_proxy_cascade::<Sys>(&mut self);
        // TLS + local-address are sourced from `.npmrc` only — pnpm
        // does not honor env vars (`NODE_EXTRA_CA_CERTS`,
        // `NODE_TLS_REJECT_UNAUTHORIZED`, etc.) for these keys
        // (Node's runtime does, but pnpm's reader does not). When
        // there is no `.npmrc`, `npmrc_auth` is the default value and
        // this is a no-op write of `TlsConfig::default()` onto the
        // already-default `self.tls`.
        npmrc_auth.apply_tls_and_local_address(&mut self);

        // Layer pnpm's global config.yaml (at `<configDir>/config.yaml`)
        // between `.npmrc` and `pnpm-workspace.yaml`, matching upstream's
        // [`index.ts:228 / 297-316`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L228).
        // Workspace-only keys are stripped inside [`WorkspaceSettings::load_global`]
        // so a user can't set `nodeLinker` or `hoist` globally — pnpm
        // rejects those in `config.yaml` and pacquet must too.
        //
        // Path-valued fields use `start_dir` as the base for relative
        // resolution — pnpm passes `workspaceDir: undefined` for the
        // global manifest, which leaves paths un-anchored. Using
        // `start_dir` here is a small pacquet-specific extension that
        // keeps relative paths well-defined; users putting absolute
        // paths (the recommended pattern) see no difference.
        //
        // `workspace_dir` is intentionally NOT set from the global
        // config — it must reflect the location of `pnpm-workspace.yaml`
        // alone. Save/restore around the call so `apply_to`'s
        // unconditional `config.workspace_dir = Some(base_dir)` write
        // doesn't leak.
        let mut virtual_store_dir_explicit = false;
        let mut global_virtual_store_dir_explicit = false;
        // `store_dir_explicit` carries the "did the user set `storeDir`
        // anywhere?" signal through the cascade. Tracked separately
        // from `virtual_store_dir_explicit` because the downstream
        // consumer is different — store_dir's late-stage cross-volume
        // resolution (port of pnpm's
        // [`storePathRelativeToHome`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L45-L78))
        // must fire only when the user has *not* pinned a path. See
        // [`crate::store_path::resolve_store_dir`].
        let mut store_dir_explicit = false;
        if let Some(global_settings) = global_settings {
            virtual_store_dir_explicit |= global_settings.virtual_store_dir.is_some();
            global_virtual_store_dir_explicit |= global_settings.global_virtual_store_dir.is_some();
            store_dir_explicit |= global_settings.store_dir.is_some();
            let saved_workspace_dir = self.workspace_dir.take();
            global_settings.apply_to(&mut self, start_dir);
            self.workspace_dir = saved_workspace_dir;
        }

        // Layer pnpm-workspace.yaml overrides on top. A missing file is
        // silent. Read or parse failures propagated while resolving
        // `workspace_yaml` above.
        //
        // Capture the "did yaml set this field" booleans *before*
        // applying yaml so the GVS derivation downstream can tell apart
        // user-pinned values from SmartDefault fallbacks. Without these
        // signals the derivation would always see populated values
        // (SmartDefault wrote them in) and would either always or never
        // re-point them, neither of which matches upstream's
        // [`extendInstallOptions.ts:343-355`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/extendInstallOptions.ts#L343-L355).
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
            //
            // `virtual_store_dir_explicit` guards the re-anchor for
            // `virtual_store_dir` — without it, a `virtualStoreDir`
            // already set in the global `config.yaml` would be
            // clobbered by the workspace-root default whenever the
            // workspace yaml itself leaves the field unset. `modules_dir`
            // needs no such guard because pnpm's `excludedPnpmKeys`
            // (and pacquet's `clear_workspace_only_fields`) keep it
            // out of the global-config surface, so it can only come
            // from workspace yaml or env vars, and env vars haven't
            // been applied yet at this point in the cascade.
            self.modules_dir = base_dir.join("node_modules");
            if !virtual_store_dir_explicit {
                self.virtual_store_dir = base_dir.join("node_modules/.pnpm");
            }
            if let Some(mut settings) = settings {
                // `|=` rather than `=` so an `enableGlobalVirtualStore` /
                // `virtualStoreDir` set in the global `config.yaml` still
                // counts as "explicitly set" when the workspace yaml
                // leaves it unset.
                virtual_store_dir_explicit |= settings.virtual_store_dir.is_some();
                global_virtual_store_dir_explicit |= settings.global_virtual_store_dir.is_some();
                store_dir_explicit |= settings.store_dir.is_some();
                settings.substitute_env_untrusted::<Sys>();
                settings.apply_to(&mut self, &base_dir);
            }
        }

        // Apply `PNPM_CONFIG_*` env vars *after* `pnpm-workspace.yaml`,
        // mirroring pnpm v11's loop at
        // [`config/reader/src/index.ts:471-488`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L471-L488):
        // env vars override yaml. The `WorkspaceSettings::apply_to`
        // call also runs the post-processing (Windows `unsafe_perm`
        // override, `hoist: false` short-circuit on `hoist_pattern`)
        // regardless of where the values came from, so env-var-set
        // values still go through the same hardening yaml-set values
        // do.
        //
        // `workspace_dir` save/restore is the same trick used for the
        // global config above — `apply_to` would otherwise clobber
        // `workspace_dir` with `start_dir`, hiding the workspace yaml's
        // location (or, if there was no yaml, setting it to a value
        // that doesn't actually correspond to a discovered workspace).
        let mut env_settings = WorkspaceSettings::from_pnpm_config_env::<Sys>();
        virtual_store_dir_explicit |= env_settings.virtual_store_dir.is_some();
        global_virtual_store_dir_explicit |= env_settings.global_virtual_store_dir.is_some();
        store_dir_explicit |= env_settings.store_dir.is_some();
        env_settings.substitute_env_trusted::<Sys>();
        let saved_workspace_dir = self.workspace_dir.clone();
        env_settings.apply_to(&mut self, start_dir);
        self.workspace_dir = saved_workspace_dir;

        // Build the per-URI auth-header lookup. Credentials were already
        // pinned to their source file's registry by `rescope_unscoped`,
        // so this is independent of the final `config.registry` (which
        // yaml may have overridden) — the security boundary holds even
        // when the workspace points the default registry elsewhere.
        npmrc_auth.build_auth_headers(&mut self);

        // Re-resolve `store_dir` against the project's volume when no
        // explicit source (global config.yaml, pnpm-workspace.yaml,
        // `PNPM_CONFIG_STORE_DIR`) set it. The SmartDefault picks
        // `<pnpm_home>/store` unconditionally; pnpm's
        // [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L14-L43)
        // probes whether `pkg_root` can hardlink into the home volume
        // and falls back to `<mountpoint>/.pnpm-store` when it can't,
        // so a workspace on a separate (case-sensitive) volume gets a
        // store on that same volume rather than the home volume.
        // Without this, typescript-eslint's case-folded path cache
        // diverges from TypeScript's case-sensitive program when the
        // workspace is case-sensitive and the home is not.
        if !store_dir_explicit && let Some(home_dir) = Sys::home_dir() {
            // `store_dir.root()` already carries the [`STORE_VERSION`]
            // suffix that [`StoreDir::from`] applied, so the
            // un-suffixed home store sits one level above. The "pnpm
            // home dir" pnpm probes against is the parent of that
            // un-suffixed home store (`~/Library/pnpm` for
            // `~/Library/pnpm/store`). Fall back to the user's actual
            // home whenever a parent is unavailable — same-volume
            // linkability is what we're after, and the home dir is on
            // the same volume as any of its children.
            let store_root_versioned = self.store_dir.root().to_path_buf();
            let store_root = store_root_versioned.parent().unwrap_or(&home_dir).to_path_buf();
            let pnpm_home_dir = store_root.parent().unwrap_or(&home_dir).to_path_buf();
            let resolved =
                store_path::resolve_store_dir::<Sys>(store_root, &pnpm_home_dir, start_dir);
            self.store_dir = StoreDir::from(resolved);
        }

        // Derive `global_virtual_store_dir` last so it sees the final
        // `store_dir` / `virtual_store_dir` after yaml has been
        // applied. An explicit `globalVirtualStoreDir` in yaml wins
        // over the derivation; otherwise the field falls back to the
        // user's pinned `virtualStoreDir` (under GVS-on) or to
        // `<store_dir>/links`. See
        // [`Self::apply_global_virtual_store_derivation`].
        self.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );

        Ok(self)
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

/// Read a `.npmrc` by explicit file path (as opposed to [`read_npmrc`],
/// which joins `.npmrc` onto a directory). Used for the `npmrcAuthFile`
/// override, which names the file directly. `None` on any read /
/// UTF-8 failure, same best-effort behaviour as [`read_npmrc`].
fn read_npmrc_file(path: &std::path::Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Port of pnpm's `readEnvVar`: read `pnpm_config_<lower>`, falling
/// back to `PNPM_CONFIG_<UPPER>`, treating an empty value as unset.
/// Used for the env vars that have to be resolved before `.npmrc` is
/// loaded (they decide *which* user-level `.npmrc` gets read).
fn read_pnpm_env<Sys: EnvVar>(lower: &str, upper: &str) -> Option<String> {
    Sys::var(&format!("pnpm_config_{lower}"))
        .or_else(|| Sys::var(&format!("PNPM_CONFIG_{upper}")))
        .filter(|value| !value.is_empty())
}

/// Port of pnpm's `readNpmEnvVar`: the `npm_config_<key>` / `NPM_CONFIG_<KEY>`
/// compatibility shim, so an `npm_config_userconfig` / `NPM_CONFIG_USERCONFIG`
/// pointing at a custom `.npmrc` (e.g. `actions/setup-node`) keeps working.
fn read_npm_env<Sys: EnvVar>(lower: &str, upper: &str) -> Option<String> {
    Sys::var(&format!("npm_config_{lower}"))
        .or_else(|| Sys::var(&format!("NPM_CONFIG_{upper}")))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod pnpm_default_parity;
#[cfg(test)]
mod tests;
