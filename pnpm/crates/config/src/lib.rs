mod api;
pub mod config_types;
mod defaults;
mod env_overlay;
mod global_bin_check;
pub mod matcher;
pub mod naming_cases;
mod npmrc_auth;
pub mod property_path;
pub mod protected_settings;
mod store_path;
pub mod version_policy;
mod workspace_yaml;

pub use crate::{
    api::{EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe},
    global_bin_check::{CheckGlobalBinDirError, check_global_bin_dir},
};

use crate::npmrc_auth::NpmrcAuth;
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
    GLOBAL_LAYOUT_VERSION, PNPM_VERSION, available_parallelism, default_config_dir,
    default_git_shallow_hosts, default_peers_suffix_max_length, default_pnpm_home_dir,
    default_unsafe_perm, default_virtual_store_dir_max_length, default_workspace_concurrency,
    is_unsafe_perm_posix, resolve_child_concurrency,
};
use crate::defaults::{
    default_cache_dir, default_child_concurrency, default_enable_global_virtual_store,
    default_fetch_retries, default_fetch_retry_factor, default_fetch_retry_maxtimeout,
    default_fetch_retry_mintimeout, default_fetch_timeout, default_hoist_pattern,
    default_modules_cache_max_age, default_modules_dir, default_public_hoist_pattern,
    default_registry, default_store_dir, default_user_agent, default_virtual_store_dir,
};
pub use workspace_yaml::{
    AuditSettings, GLOBAL_CONFIG_YAML_FILENAME, LoadWorkspaceYamlError, PackageExtension,
    PeerDependencyMeta, PeerDependencyRules, UpdateConfig, UpdateSettings,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, workspace_root_or,
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodePackageMapType {
    #[default]
    Standard,
    Loose,
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
/// The setting is `'no-downgrade' | 'off'` and drives the
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

/// What to do when the project's `packageManager` /
/// `devEngines.packageManager` field doesn't match the running pnpm.
///
/// The setting is `'download' | 'error' | 'warn' | 'ignore'`. `download`
/// switches to the pinned version, `error` aborts, `warn` prints a
/// warning, and `ignore` skips the check entirely. The documented
/// default is `download`, so [`Config::pm_on_fail`] stays optional and the
/// package-manager check applies the fallback when the setting is unset.
///
/// `pnpm with current <cmd>` runs `<cmd>` with `pmOnFail` forced to
/// [`PmOnFail::Ignore`] via the `pnpm_config_pm_on_fail` env var.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PmOnFail {
    Download,
    Error,
    Warn,
    Ignore,
}

/// What to do when a runtime declared through `devEngines.runtime` or
/// `engines.runtime` does not match the current process.
///
/// The `runtimeOnFail` setting overrides the manifest-level `onFail` value.
/// `download` reifies the runtime as a dependency; the other modes leave it
/// as an engine constraint only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeOnFail {
    Download,
    Error,
    Warn,
    Ignore,
}

impl RuntimeOnFail {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Ignore => "ignore",
        }
    }
}

/// What `pnpm run` / `pnpm exec` do when `node_modules` is out of sync
/// with the lockfile before running a script.
///
/// The setting is `'install' | 'warn' | 'error' | 'prompt' | false`
/// (default `'install'`, pnpm's `'verify-deps-before-run': 'install'`).
/// pnpm's rc type also admits a bare boolean: `true` runs the check but
/// takes none of the four actions on an out-of-sync verdict, so it is
/// modeled explicitly rather than mapped to an action.
///
/// Every script pnpm spawns gets `pnpm_config_verify_deps_before_run=false`
/// in its env, and that env var overrides every other source of this
/// setting — otherwise a script invoking `pnpm run` would re-enter the
/// check and, under `install`, recurse through the spawned install's own
/// lifecycle scripts (pnpm/pnpm#10060).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum VerifyDepsBeforeRun {
    Install,
    Warn,
    Error,
    Prompt,
    True,
    #[default]
    False,
}

impl VerifyDepsBeforeRun {
    /// Whether the deps-status check runs at all before a script.
    #[must_use]
    pub fn is_enabled(self) -> bool {
        self != VerifyDepsBeforeRun::False
    }
}

impl std::str::FromStr for VerifyDepsBeforeRun {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "install" => Ok(VerifyDepsBeforeRun::Install),
            "warn" => Ok(VerifyDepsBeforeRun::Warn),
            "error" => Ok(VerifyDepsBeforeRun::Error),
            "prompt" => Ok(VerifyDepsBeforeRun::Prompt),
            "true" => Ok(VerifyDepsBeforeRun::True),
            "false" => Ok(VerifyDepsBeforeRun::False),
            _ => Err(()),
        }
    }
}

impl serde::Serialize for VerifyDepsBeforeRun {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self {
            VerifyDepsBeforeRun::Install => serializer.serialize_str("install"),
            VerifyDepsBeforeRun::Warn => serializer.serialize_str("warn"),
            VerifyDepsBeforeRun::Error => serializer.serialize_str("error"),
            VerifyDepsBeforeRun::Prompt => serializer.serialize_str("prompt"),
            VerifyDepsBeforeRun::True => serializer.serialize_bool(true),
            VerifyDepsBeforeRun::False => serializer.serialize_bool(false),
        }
    }
}

impl<'de> serde::Deserialize<'de> for VerifyDepsBeforeRun {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl Visitor<'_> for V {
            type Value = VerifyDepsBeforeRun;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a boolean or one of "install", "warn", "error", "prompt""#)
            }
            fn visit_bool<DeError: de::Error>(self, value: bool) -> Result<Self::Value, DeError> {
                Ok(if value { VerifyDepsBeforeRun::True } else { VerifyDepsBeforeRun::False })
            }
            fn visit_str<DeError: de::Error>(self, value: &str) -> Result<Self::Value, DeError> {
                value.parse().map_err(|()| {
                    DeError::invalid_value(
                        de::Unexpected::Str(value),
                        &r#"true, false, "install", "warn", "error", or "prompt""#,
                    )
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// Minimum advisory severity shown by `pnpm audit`.
///
/// The command-level default is `low`, so [`Config::audit_level`] stays
/// optional and the audit command applies the fallback when the setting is
/// unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditLevel {
    Info,
    Low,
    Moderate,
    High,
    Critical,
}

/// `auditConfig` from `pnpm-workspace.yaml`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuditConfig {
    /// GHSA identifiers that `pnpm audit` should suppress in the rendered
    /// report.
    pub ignore_ghsas: Vec<String>,
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
/// Deserializes the `scriptsPrependNodePath: boolean | 'warn-only'`
/// yaml shape.
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
/// The setting is `linkWorkspacePackages: boolean | 'deep'`. Default is
/// [`LinkWorkspacePackages::Off`] (`'link-workspace-packages': false`).
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
    /// when resolving a bare-semver wanted dependency. The deps
    /// resolver passes the same `ResolveOptions` to every depth — the
    /// [`Self::DirectOnly`] arm only fires at the importer level
    /// (`current_depth == 0`); the caller decides which arm
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
/// The setting is `'highest' | 'time-based' | 'lowest-direct'`. Defaults to
/// [`ResolutionMode::Highest`] (`'resolution-mode': 'highest'`).
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
    /// [`Self::LowestDirect`].
    #[must_use]
    pub fn picks_lowest_direct(self) -> bool {
        matches!(self, ResolutionMode::TimeBased | ResolutionMode::LowestDirect)
    }
}

/// How `pnpm add` / `pnpm update` reconcile a directly-specified version
/// against a `catalog:` entry for the same package.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogMode {
    /// The catalog is consulted only for explicit `catalog:` specifiers;
    /// `add` / `update` never reconcile a direct version against it. The
    /// default (`'catalog-mode': 'manual'`).
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
/// onto `Config` field-by-field, following pnpm 11's split between
/// `.npmrc` (auth/registry/network) and `pnpm-workspace.yaml`
/// (project-structural settings).
#[derive(Debug, Clone, SmartDefault)]
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

    /// `extendNodePath`: when `true` (the default) and the isolated
    /// `nodeLinker` runs with a hoist pattern, command shims set
    /// `NODE_PATH` to include the hidden hoisted modules directory
    /// (`<virtual-store-dir>/node_modules`). `false` leaves `NODE_PATH`
    /// out of the shims entirely.
    #[default(true)]
    pub extend_node_path: bool,

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
    /// Default `false` — the effective default for non-`--global`
    /// installs. The `true` assignment applies only inside the
    /// `if (cliOptions['global'])` block (see
    /// `default_enable_global_virtual_store` in
    /// `crates/config/src/defaults.rs` for the full reasoning).
    /// Pacquet has no `--global` flow, so the only applicable
    /// default is `false`.
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
    /// (mostly tests).
    ///
    /// [`enable_global_virtual_store`]: Self::enable_global_virtual_store
    /// [`virtual_store_dir`]: Self::virtual_store_dir
    #[default(_code = "default_virtual_store_dir()")]
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
    /// `pacquet_package_manager::Install::run`.
    pub virtual_store_only: bool,

    /// `enableModulesDir`: pnpm's setting for suppressing the
    /// `node_modules` directory entirely. Default `true`.
    ///
    /// A `false` value (with the global virtual store off) makes the
    /// install "resolve and write the lockfile, materialize nothing" —
    /// it rides the `--lockfile-only` pipeline in
    /// `pacquet_package_manager::Install::run`. With the global virtual
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
    /// `pacquet_deps_path::create_peer_dep_graph_hash` — when the
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

    /// Scoped registry routes keyed by `@scope`, populated from
    /// `.npmrc` `@scope:registry=...` and
    /// `pnpm-workspace.yaml#registries`.
    pub registries: BTreeMap<String, String>,

    /// User-defined named-registry aliases from
    /// `pnpm-workspace.yaml#namedRegistries`. Maps each alias name
    /// (`gh`, `work`, ...) to the registry URL its `<alias>:` specifiers
    /// resolve against. Empty by default — the resolver layer merges
    /// these on top of pnpm's built-in defaults (today: `gh:` →
    /// GitHub Packages) and rejects malformed URLs at construction
    /// time with `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    ///
    /// The `namedRegistries` setting.
    pub named_registries: BTreeMap<String, String>,

    /// Resolved proxy configuration — `https-proxy`, `http-proxy`, and
    /// `no-proxy` (plus the legacy `proxy` key and env-var fallbacks),
    /// all from `.npmrc` and the process environment. The type lives
    /// in `pacquet-network` (where it is consumed by
    /// `ThrottledClient::for_installs`) because `pacquet-config`
    /// already depends on `pacquet-network` for auth-headers plumbing.
    /// Default is empty (`None` for every field) — i.e. no proxy.
    pub proxy: pacquet_network::ProxyConfig,

    /// Whether `http_proxy` came from a non-empty `http-proxy` setting,
    /// rather than falling back to the resolved HTTPS proxy.
    pub http_proxy_is_explicit: bool,

    /// Resolved TLS + `local-address` configuration — `ca`, `cafile`,
    /// `cert`, `key`, `strict-ssl`, `local-address` from `.npmrc`. The
    /// type lives in `pacquet-network` for the same reason as
    /// [`Self::proxy`]. `strict_ssl: None` here means "unset"; the
    /// `true` default is applied at client-build time by
    /// `ThrottledClient::for_installs` (`strictSsl ?? true`).
    pub tls: pacquet_network::TlsConfig,

    /// Per-registry TLS overrides — `//host[:port]/path/:ca`,
    /// `:cafile`, `:cert`, `:certfile`, `:key`, `:keyfile` from
    /// `.npmrc`. Lookup uses pnpm's 5-step nerf-darted fallback
    /// chain (exact > nerf-dart > no-port > shorter path prefix >
    /// recursive no-port retry). Per-registry fields override
    /// [`Self::tls`] field-by-field at request time (a
    /// `{ ...opts, ...sslConfig }` spread).
    pub tls_by_uri: pacquet_network::PerRegistryTls,

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
    /// `pacquet-package-manager`. No effect under
    /// `nodeLinker: isolated`.
    pub hoisting_limits: HoistingLimits,

    /// `linkWorkspacePackages` from `pnpm-workspace.yaml`. Controls
    /// whether the npm resolver consults the workspace map when
    /// resolving bare-semver wanted dependencies. See
    /// [`LinkWorkspacePackages`] for the tri-state semantics.
    /// Default `false` (`'link-workspace-packages': false`).
    pub link_workspace_packages: LinkWorkspacePackages,

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
    /// Today this only gates the `pacquet-tarball` download path;
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
    /// in flight during install — the size of the [`pacquet_network`]
    /// semaphore. The `networkConcurrency` setting; the default is the
    /// `Math.min(64, Math.max(calcMaxWorkers() * 3, 16))` formula,
    /// implemented by [`pacquet_network::default_network_concurrency`].
    #[default(_code = "pacquet_network::default_network_concurrency()")]
    pub network_concurrency: usize,

    /// Maximum number of concurrent connections (sockets) to a single
    /// registry origin — the `maxSockets` setting, mirroring undici's
    /// per-origin `connections` cap that pnpm applies. `None` (the default)
    /// leaves the per-origin socket count bounded only by
    /// [`Self::network_concurrency`]; `Some(n)` additionally caps each
    /// `scheme://host[:port]` at `n` in-flight sockets, queueing the rest.
    pub max_sockets: Option<usize>,

    /// Per-request network timeout in milliseconds. The `fetchTimeout`
    /// setting (default `60000` — 60 s, see
    /// [`pacquet_network::DEFAULT_FETCH_TIMEOUT_MS`]). Applied as both
    /// the response and connect deadline of the reqwest client.
    #[default(_code = "default_fetch_timeout()")]
    pub fetch_timeout: u64,

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

    /// Path to the user-level `.npmrc` to read auth from, overriding the
    /// default `~/.npmrc`. The `npmrcAuthFile` setting (and the
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
    /// yaml — keeping `PATCH_KEY_CONFLICT` diagnostics aligned.
    ///
    /// pnpm v11 reads `patchedDependencies` from `pnpm-workspace.yaml`
    /// only.
    pub patched_dependencies: Option<IndexMap<String, String>>,

    /// Raw `patchesDir` setting used by `patch-commit` when writing
    /// generated patch files. `None` means the command default
    /// (`patches`) applies.
    pub patches_dir: Option<String>,

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
    /// in `pacquet-package-manager`.
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
    /// `extraEnv` object, wired up in `pacquet_cli`'s
    /// `run_update_config_hooks`. That hook runs only for the
    /// install-family commands (install, deploy, dedupe, prune), so this
    /// is non-empty only under those; other commands' spawn sites read it
    /// too, but see an empty map until the hook broadens.
    pub extra_env: HashMap<String, String>,

    /// `unsafePerm` from `pnpm-workspace.yaml`. When `false`,
    /// lifecycle scripts run under a TMPDIR isolated to
    /// `node_modules/.tmp` and uid/gid drops to a non-root user.
    /// Pacquet honors the TMPDIR side (see
    /// `pacquet_executor::make_env`); the uid/gid drop is a no-op in
    /// practice because the npm-lifecycle fork never populates
    /// `opts.user` / `opts.group`, so it just re-applies the current
    /// process's uid/gid.
    ///
    /// The default is auto-detected via [`default_unsafe_perm`]:
    /// `true` on Windows or POSIX-not-root; `false` when running
    /// as root on POSIX. On Windows,
    /// [`WorkspaceSettings::apply_to`] also force-overrides the
    /// applied value to `true` regardless of yaml — a
    /// `process.platform === 'win32'` gate.
    #[default(_code = "default_unsafe_perm()")]
    pub unsafe_perm: bool,

    /// `childConcurrency` from `pnpm-workspace.yaml` — the maximum
    /// number of lifecycle-script spawns that may run in parallel
    /// inside a single `BuildModules` chunk. Resolved through
    /// [`resolve_child_concurrency`] so the yaml value can be
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
    /// [`resolve_child_concurrency`] so a non-positive yaml/CLI value is
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
    /// `pacquet-workspace-projects-filter`. A CLI-only array: not a
    /// `.npmrc` / `pnpm-workspace.yaml` key, so only the CLI layer
    /// populates it.
    pub filter: Vec<String>,

    /// `--filter-prod` selectors. Same shape as [`Self::filter`], but
    /// each selector follows production dependencies only when its
    /// dependency walk runs. A CLI-only array.
    pub filter_prod: Vec<String>,

    /// `testPattern` from `pnpm-workspace.yaml` /
    /// `PNPM_CONFIG_TEST_PATTERN`, overridable by the `--test-pattern`
    /// CLI flag. Glob patterns naming test files: when a `[<since>]`
    /// changed-packages filter selects a project whose changed files
    /// all match, the project is selected without its dependents.
    pub test_pattern: Vec<String>,

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
    /// `pacquet-package-manager`'s `InstallabilityHost`, downstream of
    /// this crate) so optional platform-tagged dependencies for the
    /// listed `os` / `cpu` / `libc` values are kept even when they
    /// don't match the host triple. Per-axis CLI flags (`--cpu`,
    /// `--libc`, `--os`) override individual axes.
    /// Default `None` so the host triple is the sole accept set
    /// when neither yaml nor CLI sets a value.
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,

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

    /// `versioning` from `pnpm-workspace.yaml`: native workspace release
    /// management, consumed by `pnpm change` and the bare `pnpm version -r`.
    pub versioning: pacquet_versioning::VersioningSettings,

    /// Glob-style `name[@version]` patterns that opt specific packages
    /// out of the [`trust_policy`] check. The `trustPolicyExclude`
    /// setting.
    ///
    /// [`trust_policy`]: Self::trust_policy
    pub trust_policy_exclude: Option<Vec<String>>,

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
    /// (`add`, `remove`, `update`) also drop catalog entries that no
    /// workspace project references. The `cleanupUnusedCatalogs`
    /// setting; default `false`, matching pnpm.
    pub cleanup_unused_catalogs: bool,

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
    /// [`CatalogMode::Manual`]. The `saveCatalogName` setting (default
    /// `undefined`). A CLI-only flag, so pacquet does not read it from
    /// `pnpm-workspace.yaml`; the effective value is threaded onto the
    /// `add` command from the CLI.
    pub save_catalog_name: Option<String>,

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
    /// fetchers via [`pacquet_network::AuthHeaders::for_url`]. Empty
    /// when no `.npmrc` was found or no auth keys were set.
    pub auth_headers: std::sync::Arc<pacquet_network::AuthHeaders>,

    /// Raw `_authToken` values keyed by the nerf-darted registry URI
    /// (`//host[:port]/path/`), for the default (registry-wide) scope.
    /// Unlike [`Self::auth_headers`], which bakes credentials into
    /// ready-to-send `Authorization` header values and discards the
    /// raw token, this preserves the unmodified token so commands like
    /// `pnpm logout` can read it back to revoke it on the registry.
    /// The subset of raw auth config the auth commands consult.
    pub auth_tokens_by_uri: std::collections::HashMap<String, String>,

    pub package_manager_bootstrap: PackageManagerBootstrap,

    /// Camel-cased record of the settings the user *explicitly* set through
    /// `pnpm-workspace.yaml`, the global `config.yaml`, and `PNPM_CONFIG_*`
    /// env vars (with `_auth` excluded and `null` values dropped). Populated
    /// by [`Config::current`]; empty when a `Config` is built without it.
    ///
    /// This tracks the explicitly-set keys plus the merged config record
    /// consumed by `pnpm config get` / `pnpm config list`:
    /// because [`WorkspaceSettings`]'s fields are `Option`s, a serialized
    /// settings struct names exactly the keys a source set, with the user's
    /// raw value. The `config` command turns this into the record it prints.
    pub explicit_settings: serde_json::Map<String, serde_json::Value>,

    /// Raw `.npmrc` / `auth.ini` config keys (those for which
    /// [`config_types::is_ini_config_key`] holds: `registry`, `@scope:registry`,
    /// `//host/:_authToken`, `username`, `ca`, ...), post-`${VAR}` substitution
    /// and merged across sources. The raw auth-config map, consumed by
    /// `pnpm config get` / `pnpm config list`.
    pub raw_auth_config: BTreeMap<String, String>,

    /// The global pnpm config directory (`<configDir>`), where `config.yaml`
    /// and `auth.ini` live. `None` when it cannot be determined. Consumed by
    /// `pnpm config` and by `globalconfig` lookups.
    pub config_dir: Option<PathBuf>,
}

/// Registry + network configuration for resolving the package manager pnpm
/// auto-switches to. Built only from sources outside the repository's
/// control (builtin default, user `.npmrc`, `auth.ini`, URL-scoped env), so
/// a malicious `pnpm-workspace.yaml` or project `.npmrc` cannot redirect the
/// package-manager bytes to an attacker registry or proxy. See
/// GHSA-j2hc-m6cf-6jm8.
#[derive(Debug, Clone, SmartDefault)]
pub struct PackageManagerBootstrap {
    /// Defaults to the public npm registry so a [`Config`] built without
    /// [`Config::current`] never resolves against an empty registry.
    #[default(_code = "default_registry()")]
    pub registry: String,
    /// Scoped registry routes (keyed by `@scope`), excluding `default`.
    pub registries: BTreeMap<String, String>,
    pub proxy: pacquet_network::ProxyConfig,
    /// Whether `proxy.http_proxy` came from a non-empty trusted
    /// `http-proxy` setting rather than the HTTPS-proxy fallback.
    pub http_proxy_is_explicit: bool,
    pub tls: pacquet_network::TlsConfig,
    pub tls_by_uri: pacquet_network::PerRegistryTls,
    pub auth_headers: std::sync::Arc<pacquet_network::AuthHeaders>,
}

impl PackageManagerBootstrap {
    /// Registry map in pnpm's `Registries` shape: `default` plus the
    /// configured scoped routes. Mirrors [`Config::resolved_registries`].
    #[must_use]
    pub fn resolved_registries(&self) -> BTreeMap<String, String> {
        let mut registries = self.registries.clone();
        registries.insert("default".to_string(), self.registry.clone());
        registries
    }
}

impl Config {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overlays proxy options after file and environment settings have been
    /// resolved. An HTTPS proxy also serves HTTP unless a lower-priority source
    /// explicitly configured the HTTP proxy.
    pub fn apply_proxy_cli_overrides(
        &mut self,
        https_proxy: Option<&str>,
        http_proxy: Option<&str>,
        no_proxy: Option<&str>,
    ) {
        for (proxy, http_proxy_is_explicit) in [
            (&mut self.proxy, self.http_proxy_is_explicit),
            (
                &mut self.package_manager_bootstrap.proxy,
                self.package_manager_bootstrap.http_proxy_is_explicit,
            ),
        ] {
            if let Some(value) = https_proxy {
                proxy.https_proxy = Some(value.to_string());
                if http_proxy.is_none() && !http_proxy_is_explicit {
                    proxy.http_proxy = Some(value.to_string());
                }
            }
            if let Some(value) = http_proxy {
                proxy.http_proxy = Some(value.to_string());
            }
            if let Some(value) = no_proxy {
                proxy.no_proxy = Some(crate::npmrc_auth::parse_no_proxy(value));
            }
        }
    }

    /// Effective value of [`Self::minimum_release_age_strict`].
    /// Returns the user-supplied value when set, else `false`.
    ///
    /// pnpm flips this to `true` when the user *explicitly* set
    /// `minimumReleaseAge`, but the "explicitly set vs default" check
    /// relies on an `explicitlySetKeys` tracker that pacquet's config
    /// layer doesn't have yet. Without that, distinguishing the built-in
    /// 1440-minute default from a user-typed `minimumReleaseAge: 1440`
    /// isn't possible, so this resolver stays conservative: explicit
    /// `true` / `false` from yaml wins, otherwise `false`. The verifier
    /// itself doesn't gate on this flag — it's resolver-only — so
    /// the conservative default is dormant until pacquet grows the
    /// resolver and the `explicitlySetKeys` mechanism alongside it.
    pub fn resolved_minimum_release_age_strict(&self) -> bool {
        self.minimum_release_age_strict.unwrap_or(false)
    }

    /// Effective [`Self::minimum_release_age`], with `Some(0)` treated
    /// as "disabled" (`None`).
    ///
    /// A falsy check: `minimumReleaseAge: 0` disables the maturity
    /// cutoff. Disabling it is also what makes `resolutionMode:
    /// lowest-direct` / `time-based` observable for direct dependencies
    /// — while a cutoff is active the picker always prefers the highest
    /// mature version, overriding the lowest-version pick.
    pub fn resolved_minimum_release_age(&self) -> Option<u64> {
        self.minimum_release_age.filter(|&minutes| minutes > 0)
    }

    /// Whether version resolution must fetch the full packument to obtain
    /// per-version `time` and trust evidence.
    ///
    /// The `no-downgrade` trust check reads per-version trust evidence
    /// (`_npmUser` / `dist.attestations`) that the abbreviated packument
    /// *never* carries — `registrySupportsTimeField` only concerns the
    /// `time` field — so it always requires the full packument. Time-based
    /// resolution needs only `time`, which abbreviated metadata carries
    /// when the registry advertises it, so it is gated on
    /// `!registrySupportsTimeField`.
    ///
    /// `minimumReleaseAge` is intentionally absent: the resolver upgrades
    /// abbreviated metadata to full on demand for the maturity check (see
    /// `maybe_upgrade_abbreviated_meta_for_release_age`), so it doesn't
    /// need the full packument requested up front.
    ///
    /// The install resolver (`PickPolicy`), `pacquet add`'s pre-resolution,
    /// and the `self-update` / `pnpm with` engine probe all derive their
    /// metadata mode from here so none of them can drift.
    #[must_use]
    pub fn requires_full_metadata_for_resolution(&self) -> bool {
        self.trust_policy == TrustPolicy::NoDowngrade
            || (self.resolution_mode == ResolutionMode::TimeBased
                && !self.registry_supports_time_field)
    }

    /// Registry map in pnpm's `Registries` shape: `default` plus the
    /// configured scoped routes keyed by `@scope`.
    #[must_use]
    pub fn resolved_registries(&self) -> BTreeMap<String, String> {
        let mut registries = self.registries.clone();
        registries.insert("default".to_string(), self.registry.clone());
        registries
    }

    /// Whether the install should consult the side-effects cache
    /// (`sideEffectsCacheRead = sideEffectsCache ?? sideEffectsCacheReadonly`).
    ///
    /// Pacquet collapses pnpm's tri-state (`undefined`/`true`/`false`)
    /// into two booleans: the cache is read when either flag is on, so
    /// users who only want the READ side can set
    /// `sideEffectsCacheReadonly: true` with `sideEffectsCache: false`
    /// and get a read-only view.
    pub fn side_effects_cache_read(&self) -> bool {
        self.side_effects_cache || self.side_effects_cache_readonly
    }

    /// Whether the install is allowed to populate the side-effects
    /// cache after a successful postinstall
    /// (`sideEffectsCacheWrite = sideEffectsCache`), with the additional
    /// constraint that the explicit `sideEffectsCacheReadonly: true`
    /// always wins — a `??` would let `readonly` slip through when both
    /// flags are explicitly set, but `readonly` as a flag name only makes
    /// sense if it really does block writes.
    pub fn side_effects_cache_write(&self) -> bool {
        self.side_effects_cache && !self.side_effects_cache_readonly
    }

    /// Resolve relative patch file paths in
    /// [`Config::patched_dependencies`] against
    /// [`Config::workspace_dir`], compute SHA-256 hashes, and bucket
    /// the entries into a [`PatchGroupRecord`].
    ///
    /// Resolves each configured patch path against the workspace dir,
    /// then hashes the files.
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
    /// Pacquet diverges from pnpm on *which* field carries the GVS path:
    ///
    /// - **pnpm**: mutates `virtualStoreDir` in place when GVS is
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
    /// `InstallWithFreshLockfile` path that pnpm doesn't have.
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
    /// to `<store_dir>/links`, an unconditional
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

    /// Clear both hoist patterns when [`virtual_store_only`] is set.
    ///
    /// A `virtualStoreOnly` install does no hoisting, so the patterns it
    /// records in `.modules.yaml` must be empty — that is how the next
    /// ordinary install learns hoisting still has to be done from
    /// scratch rather than reading a pattern it never applied.
    ///
    /// [`virtual_store_only`]: Self::virtual_store_only
    pub fn apply_virtual_store_only_derivation(&mut self) {
        if !self.virtual_store_only {
            return;
        }
        self.hoist_pattern = Some(Vec::new());
        self.public_hoist_pattern = Some(Vec::new());
    }

    /// Restore the smart default store after a higher-precedence config
    /// source explicitly clears `storeDir`.
    pub fn reset_store_dir_to_default<Sys>(&mut self, start_dir: &Path)
    where
        Sys: EnvVar + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        self.store_dir = default_store_dir::<Sys>();
        self.resolve_default_store_dir::<Sys>(start_dir);
        self.explicit_settings.remove("storeDir");
        let virtual_store_dir_explicit = self.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            self.explicit_settings.contains_key("globalVirtualStoreDir");
        self.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }

    fn resolve_default_store_dir<Sys: GetHomeDir + LinkProbe>(&mut self, start_dir: &Path) {
        let Some(home_dir) = Sys::home_dir() else {
            return;
        };
        // `store_dir.root()` includes the layout version, so its parent is
        // the unversioned store and the next parent is pnpm's home directory.
        // The linkability probe only cares about that directory's volume;
        // fall back to the user's home when either parent is unavailable.
        let store_root_versioned = self.store_dir.root().to_path_buf();
        let store_root = store_root_versioned.parent().unwrap_or(&home_dir).to_path_buf();
        let pnpm_home_dir = store_root.parent().unwrap_or(&home_dir).to_path_buf();
        let resolved = store_path::resolve_store_dir::<Sys>(store_root, &pnpm_home_dir, start_dir);
        self.store_dir = StoreDir::from(resolved);
    }

    /// Return the `virtualStoreDir` value pnpm exposes externally — the
    /// path written into `.modules.yaml` and emitted in the `pnpm:context`
    /// NDJSON event.
    ///
    /// pnpm mutates `virtualStoreDir` in place when
    /// `enableGlobalVirtualStore` is on and the user hasn't pinned
    /// `virtualStoreDir`, so every consumer that reads `ctx.virtualStoreDir`
    /// — including the modules-manifest writer and the `pnpm:context`
    /// debug log — sees the GVS-derived path.
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

    /// Load the merged configuration for a CLI run.
    ///
    /// Config sources (low → high precedence): `SmartDefault`, the supported
    /// `.npmrc` subset (cwd, falling back to home), global `config.yaml`,
    /// project `pnpm-workspace.yaml`, then `PNPM_CONFIG_*` env.
    ///
    /// Pacquet currently applies `registry`, scoped registry routes,
    /// npm-auth credentials, the
    /// proxy keys (`https-proxy`, `http-proxy`, `proxy`, `no-proxy` /
    /// `noproxy`), and the TLS + local-address keys (`ca`, `cafile`,
    /// `cert`, `key`, `strict-ssl`, `local-address`) from `.npmrc`.
    /// Other `.npmrc` entries — project-structural settings like
    /// `storeDir`, `lockfile` and `hoist-pattern` — are silently
    /// ignored here. Those must come from `pnpm-workspace.yaml` or CLI
    /// flags, matching pnpm 11.
    ///
    /// Returns [`LoadWorkspaceYamlError`] when an existing
    /// `pnpm-workspace.yaml` cannot be read or parsed. A missing file is not
    /// an error.
    pub fn current<Sys>(self, start_dir: &std::path::Path) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        self.current_inner::<Sys>(start_dir, false)
    }

    /// Like [`Config::current`], but the project `pnpm-workspace.yaml` does
    /// not contribute the `minimumReleaseAge` / `trustPolicy` policies — see
    /// [`WorkspaceSettings::clear_self_update_policy`].
    pub fn current_for_self_update<Sys>(
        self,
        start_dir: &std::path::Path,
    ) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        self.current_inner::<Sys>(start_dir, true)
    }

    fn current_inner<Sys>(
        mut self,
        start_dir: &std::path::Path,
        for_self_update: bool,
    ) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
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
        // [`NpmrcAuth::rescope_unscoped`].
        //
        // The global `config.yaml` is loaded up front: its `npmrcAuthFile`
        // participates in the user-level path resolution below, and its
        // directory is where `auth.ini` lives.
        let global_config_dir = default_config_dir::<Sys>();
        self.config_dir.clone_from(&global_config_dir);
        let mut global_settings =
            global_config_dir.as_deref().map(WorkspaceSettings::load_global).transpose()?.flatten();
        if let Some(global_settings) = global_settings.as_mut() {
            global_settings.substitute_env_trusted::<Sys>();
        }

        // Resolve the workspace dir before reading the project `.npmrc`
        // so subdirectory invocations use the workspace-root config:
        // the workspace dir, falling back to the local prefix.
        let env_workspace_dir = Sys::var_os("NPM_CONFIG_WORKSPACE_DIR")
            .or_else(|| Sys::var_os("npm_config_workspace_dir"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let workspace_yaml = if let Some(env_dir) = env_workspace_dir {
            // Env-var path: load yaml directly from the env dir. A
            // missing file is silent, but the re-anchor still fires
            // because the user has explicitly told us where the
            // workspace lives.
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

        // Resolve the user-level `.npmrc` path. Precedence:
        // the `npmrc_auth_file` field (CLI `--npmrc-auth-file` /
        // `--userconfig`) > `PNPM_CONFIG_NPMRC_AUTH_FILE` >
        // `PNPM_CONFIG_USERCONFIG` > global `config.yaml`'s `npmrcAuthFile`
        // > `npm_config_userconfig`. Each env var is empty-filtered
        // individually (a `value !== ''` check).
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
            let mut auth = NpmrcAuth::from_ini::<Sys>(&text, &dir);
            auth.rescope_unscoped(label);
            auth
        };
        let project_npmrc_dir =
            workspace_yaml.as_ref().map_or(start_dir, |(base_dir, _)| base_dir.as_path());
        let project_npmrc_path = project_npmrc_dir.join(".npmrc");
        // When npmrcAuthFile explicitly points at the project .npmrc, the user has
        // opted in to trusting it — allow auth env expansion and suppress the warning.
        // A relative value (e.g. `PNPM_CONFIG_NPMRC_AUTH_FILE=.npmrc`) is anchored
        // at the cwd — where the user-level read below actually reads it from, and
        // how pnpm's `path.resolve` anchors it.
        let project_is_trusted_auth_file = user_npmrc_path.as_deref().is_some_and(|user| {
            if user.is_absolute() {
                user == project_npmrc_path
            } else {
                Sys::current_dir().is_ok_and(|cwd| cwd.join(user) == project_npmrc_path)
            }
        });
        let project_source = read_npmrc(project_npmrc_dir).map(|text| {
            let mut auth = if project_is_trusted_auth_file {
                NpmrcAuth::from_ini::<Sys>(&text, project_npmrc_dir)
            } else {
                NpmrcAuth::from_project_ini::<Sys>(&text, project_npmrc_dir)
            };
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

        // URL-scoped credentials from `npm_config_//...` / `pnpm_config_//...`
        // environment variables. These are trusted (they come from the
        // environment, not the repository) and host-scoped by construction, so
        // they sit at the top of the precedence chain — above the project
        // `.npmrc` — following the env-over-workspace ordering.
        let env_scoped_source = {
            let auth = NpmrcAuth::from_url_scoped_env::<Sys>();
            (!auth.creds_by_scope_by_uri.is_empty()).then_some(auth)
        };

        // Structured `_auth` registry auth from its two trusted sources:
        // the `pnpm_config__auth` env var and the global `config.yaml`'s
        // `_auth` key (env wins on conflict). See `from_json_sources`.
        let json_auth = global_settings
            .as_ref()
            .and_then(|settings| settings.auth.as_ref())
            .pipe(NpmrcAuth::from_json_sources::<Sys>)
            .map_err(|source| LoadWorkspaceYamlError::InvalidJsonAuth { source })?;
        let json_auth_has_content = !json_auth.creds_by_scope_by_uri.is_empty()
            || !json_auth.json_env_registries.is_empty();
        let env_json_source = json_auth_has_content.then_some(json_auth);

        // Capture the trusted sources (everything but `project_source`) for
        // [`PackageManagerBootstrap`] before the fold below consumes them.
        let trusted_sources = [
            env_json_source.clone(),
            env_scoped_source.clone(),
            auth_ini_source.clone(),
            user_source.clone(),
        ];

        // Fold high-priority-first: the first present source is the
        // base, each lower source fills the gaps it left
        // ([`NpmrcAuth::merge_under`]). `env_json_source` is listed before
        // `env_scoped_source` so the JSON env var wins on the rare occasion
        // both define the same `//host/:_authToken` key — the JSON auth is
        // applied after the env-scoped config, so it wins.
        let mut sources =
            [env_json_source, env_scoped_source, project_source, auth_ini_source, user_source]
                .into_iter()
                .flatten();
        let mut npmrc_auth = sources.next().unwrap_or_default();
        for lower in sources {
            npmrc_auth.merge_under(lower);
        }
        self.http_proxy_is_explicit = has_nonempty_string(npmrc_auth.http_proxy.as_deref());
        // Retain the merged raw `.npmrc` / `auth.ini` config keys for
        // `pnpm config get` / `pnpm config list` before the structured fields
        // are consumed below.
        self.raw_auth_config = std::mem::take(&mut npmrc_auth.raw_ini_config);

        let mut trusted_sources = trusted_sources.into_iter().flatten();
        let mut trusted_auth = trusted_sources.next().unwrap_or_default();
        for lower in trusted_sources {
            trusted_auth.merge_under(lower);
        }

        // A `tokenHelper` names an executable, so it is honored only from a
        // trusted, non-repo source. Reject one that a workspace or project
        // `.npmrc` contributed by comparing the full merge against the
        // trusted-only merge before either is consumed below.
        crate::npmrc_auth::enforce_token_helper_trust(&npmrc_auth, &trusted_auth)?;

        self.package_manager_bootstrap = build_package_manager_bootstrap::<Sys>(trusted_auth)?;
        if let Some(global_settings) = global_settings.as_ref() {
            self.package_manager_bootstrap.http_proxy_is_explicit |=
                has_nonempty_string(global_settings.http_proxy.as_deref());
            let http_proxy_is_explicit = self.package_manager_bootstrap.http_proxy_is_explicit;
            global_settings
                .apply_proxy_to(&mut self.package_manager_bootstrap.proxy, http_proxy_is_explicit);
        }

        npmrc_auth.apply_registry_and_warn(&mut self);
        // Proxy cascade fires unconditionally — even when no `.npmrc`
        // is found — because the env-var fallback is a normalization step
        // on the resolved config, not a function of `.npmrc` presence.
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
        // between `.npmrc` and `pnpm-workspace.yaml`.
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
        // resolution must fire only when the user has *not* pinned a
        // path. See [`crate::store_path::resolve_store_dir`].
        let mut store_dir_explicit = false;
        if let Some(global_settings) = global_settings {
            self.http_proxy_is_explicit |=
                has_nonempty_string(global_settings.http_proxy.as_deref());
            virtual_store_dir_explicit |= global_settings.virtual_store_dir.is_some();
            global_virtual_store_dir_explicit |= global_settings.global_virtual_store_dir.is_some();
            store_dir_explicit |= global_settings.store_dir.is_some();
            collect_explicit_settings(&mut self.explicit_settings, &global_settings);
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
        // re-point them, neither of which is correct.
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
            // The workspace root is structural context (env-lockfile reads/
            // writes, pin persistence), not a "setting" — set it whenever a
            // workspace is discovered, even on the `NPM_CONFIG_WORKSPACE_DIR`
            // path when the yaml file is missing and `apply_to` (which also
            // writes it) never runs.
            self.workspace_dir = Some(base_dir.clone());
            if let Some(mut settings) = settings {
                // `|=` rather than `=` so an `enableGlobalVirtualStore` /
                // `virtualStoreDir` set in the global `config.yaml` still
                // counts as "explicitly set" when the workspace yaml
                // leaves it unset.
                virtual_store_dir_explicit |= settings.virtual_store_dir.is_some();
                global_virtual_store_dir_explicit |= settings.global_virtual_store_dir.is_some();
                store_dir_explicit |= settings.store_dir.is_some();
                settings.substitute_env_untrusted::<Sys>();
                self.http_proxy_is_explicit |= has_nonempty_string(settings.http_proxy.as_deref());
                if for_self_update {
                    settings.clear_self_update_policy();
                }
                collect_explicit_settings(&mut self.explicit_settings, &settings);
                settings.apply_to(&mut self, &base_dir);
            }
        }

        // Apply `_auth` routes after workspace yaml (so they win over
        // repo-controlled registries) but before `PNPM_CONFIG_*` (so an
        // explicit `pnpm_config_registry` / `--registry` still wins) —
        // pnpm's "CLI > _auth > yaml" precedence.
        npmrc_auth.apply_json_env_registries(&mut self);

        // Apply `PNPM_CONFIG_*` env vars *after* `pnpm-workspace.yaml`:
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
        // `PNPM_CONFIG_REGISTRY` comes from the environment, not the
        // repository, so it overrides the bootstrap default registry too.
        let env_registry_override = env_settings.registry.clone();
        let env_http_proxy_is_explicit = has_nonempty_string(env_settings.http_proxy.as_deref());
        self.http_proxy_is_explicit |= env_http_proxy_is_explicit;
        self.package_manager_bootstrap.http_proxy_is_explicit |= env_http_proxy_is_explicit;
        collect_explicit_settings(&mut self.explicit_settings, &env_settings);
        let bootstrap_http_proxy_is_explicit =
            self.package_manager_bootstrap.http_proxy_is_explicit;
        env_settings.apply_proxy_to(
            &mut self.package_manager_bootstrap.proxy,
            bootstrap_http_proxy_is_explicit,
        );
        let saved_workspace_dir = self.workspace_dir.clone();
        env_settings.apply_to(&mut self, start_dir);
        self.workspace_dir = saved_workspace_dir;
        if let Some(registry) = env_registry_override {
            let normalized =
                if registry.ends_with('/') { registry } else { format!("{registry}/") };
            self.registries.insert("default".to_string(), normalized.clone());
            self.package_manager_bootstrap.registry.clone_from(&normalized);
            self.package_manager_bootstrap.registries.insert("default".to_string(), normalized);
        }

        // Build the per-URI auth-header lookup. Credentials were already
        // pinned to their source file's registry by `rescope_unscoped`,
        // so this is independent of the final `config.registry` (which
        // yaml may have overridden) — the security boundary holds even
        // when the workspace points the default registry elsewhere.
        npmrc_auth.build_auth_headers(&mut self)?;

        // Re-resolve `store_dir` against the project's volume when no
        // explicit source (global config.yaml, pnpm-workspace.yaml,
        // `PNPM_CONFIG_STORE_DIR`) set it. The SmartDefault picks
        // `<pnpm_home>/store` unconditionally; the store-path resolution
        // probes whether `pkg_root` can hardlink into the home volume
        // and falls back to `<mountpoint>/.pnpm-store` when it can't,
        // so a workspace on a separate (case-sensitive) volume gets a
        // store on that same volume rather than the home volume.
        // Without this, typescript-eslint's case-folded path cache
        // diverges from TypeScript's case-sensitive program when the
        // workspace is case-sensitive and the home is not.
        if !store_dir_explicit {
            self.resolve_default_store_dir::<Sys>(start_dir);
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

        self.apply_virtual_store_only_derivation();

        // Resolve the global install directories:
        // `globalPkgDir = (globalDir ?? <pnpm-home>/global)/v11` and
        // `bin = globalBinDir ?? <pnpm-home>/bin`.
        if self.global_dir.is_none() {
            self.global_dir = read_pnpm_env::<Sys>("global_dir", "GLOBAL_DIR").map(PathBuf::from);
        }
        if self.global_bin_dir.is_none() {
            self.global_bin_dir =
                read_pnpm_env::<Sys>("global_bin_dir", "GLOBAL_BIN_DIR").map(PathBuf::from);
        }
        let pnpm_home_dir = default_pnpm_home_dir::<Sys>();
        let global_dir_root = self
            .global_dir
            .clone()
            .or_else(|| pnpm_home_dir.as_ref().map(|home| home.join("global")));
        self.global_pkg_dir = global_dir_root.map(|root| root.join(GLOBAL_LAYOUT_VERSION));
        self.global_bin = self
            .global_bin_dir
            .clone()
            .or_else(|| pnpm_home_dir.as_ref().map(|home| home.join("bin")));

        // Inside a workspace, scripts and `pnpm exec` also get the
        // workspace root's `node_modules/.bin` on PATH — pnpm's
        // `extraBinPaths = [join(workspaceDir, 'node_modules', '.bin')]`.
        self.extra_bin_paths = self
            .workspace_dir
            .as_deref()
            .map(|dir| vec![dir.join("node_modules").join(".bin")])
            .unwrap_or_default();

        Ok(self)
    }

    /// Persist the config data until the program terminates.
    pub fn leak(self) -> &'static mut Self {
        self.pipe(Box::new).pipe(Box::leak)
    }
}

/// Fold a source's explicitly-set settings into the running record.
///
/// Serializes `settings` to a camelCase JSON object (its `Option` fields make
/// a serialized value name exactly the keys this source set) and copies every
/// non-`null` entry into `target`, later sources overriding earlier ones. The
/// `_auth` key is dropped — it carries credentials and never belongs in
/// `pnpm config list` output (raw auth keys come from `raw_auth_config`,
/// censored at render time).
fn collect_explicit_settings(
    target: &mut serde_json::Map<String, serde_json::Value>,
    settings: &WorkspaceSettings,
) {
    let Ok(serde_json::Value::Object(map)) = serde_json::to_value(settings) else {
        return;
    };
    for (key, value) in map {
        if key == "_auth" || value.is_null() {
            continue;
        }
        target.insert(key, value);
    }
}

fn has_nonempty_string(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// Build the [`PackageManagerBootstrap`] from the already-folded trusted
/// sources, running them through the same registry/proxy/TLS/auth steps the
/// full config uses so the bootstrap cascade matches the project cascade
/// minus the repository-controlled sources.
fn build_package_manager_bootstrap<Sys: EnvVar>(
    mut trusted_auth: NpmrcAuth,
) -> Result<PackageManagerBootstrap, LoadWorkspaceYamlError> {
    // The full-config fold already surfaced these sources' `${VAR}` warnings;
    // drop the duplicates this second pass would log.
    trusted_auth.warnings.clear();
    let http_proxy_is_explicit = has_nonempty_string(trusted_auth.http_proxy.as_deref());
    let mut config = Config::default();
    trusted_auth.apply_registry_and_warn(&mut config);
    trusted_auth.apply_json_env_registries(&mut config);
    trusted_auth.apply_proxy_cascade::<Sys>(&mut config);
    trusted_auth.apply_tls_and_local_address(&mut config);
    trusted_auth.build_auth_headers(&mut config)?;
    Ok(PackageManagerBootstrap {
        registry: config.registry,
        registries: config.registries,
        proxy: config.proxy,
        http_proxy_is_explicit,
        tls: config.tls,
        tls_by_uri: config.tls_by_uri,
        auth_headers: config.auth_headers,
    })
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

/// Read `pnpm_config_<lower>`, falling back to `PNPM_CONFIG_<UPPER>`,
/// treating an empty value as unset. Used for the env vars that have to
/// be resolved before `.npmrc` is loaded (they decide *which*
/// user-level `.npmrc` gets read).
fn read_pnpm_env<Sys: EnvVar>(lower: &str, upper: &str) -> Option<String> {
    Sys::var(&format!("pnpm_config_{lower}"))
        .or_else(|| Sys::var(&format!("PNPM_CONFIG_{upper}")))
        .filter(|value| !value.is_empty())
}

/// The `npm_config_<key>` / `NPM_CONFIG_<KEY>` compatibility shim, so an
/// `npm_config_userconfig` / `NPM_CONFIG_USERCONFIG` pointing at a custom
/// `.npmrc` (e.g. `actions/setup-node`) keeps working.
fn read_npm_env<Sys: EnvVar>(lower: &str, upper: &str) -> Option<String> {
    Sys::var(&format!("npm_config_{lower}"))
        .or_else(|| Sys::var(&format!("NPM_CONFIG_{upper}")))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod pnpm_default_parity;
#[cfg(test)]
mod tests;
