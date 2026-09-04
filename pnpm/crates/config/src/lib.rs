mod api;
pub mod config_types;
mod defaults;
mod env_overlay;
pub mod esm_node_path_loader;
mod global_bin_check;
pub mod known_settings;
pub mod matcher;
pub mod naming_cases;
mod npmrc_auth;
mod override_version_references;
pub mod property_path;
pub mod protected_settings;
pub mod proxy_keys;
pub mod refused_keys;
mod store_path;
pub mod version_policy;
mod workspace_yaml;

pub use crate::{
    api::{EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe},
    global_bin_check::{CheckGlobalBinDirError, check_global_bin_dir},
    npmrc_auth::{is_json_auth_scope, validate_json_auth_registry},
};

use crate::{matcher::create_matcher, npmrc_auth::NpmrcAuth};
use indexmap::IndexMap;
use pipe_trait::Pipe;
use pnpm_git_utils::{Host as GitHost, get_current_branch};
use pnpm_lockfile::{Lockfile, RegistryOptions, WantedLockfileSelection};
use pnpm_patching::{
    CalcPatchHashError, PatchGroupRecord, PatchInput, ResolvePatchedDependenciesError,
    create_hex_hash_from_file, group_patched_dependencies, resolve_and_group,
};
use pnpm_store_dir::StoreDir;
use pnpm_workspace_state::ConfigDependency;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub use crate::defaults::{
    BUILTIN_REGISTRIES_BY_PREFIX, DEFAULT_JSR_REGISTRY, GLOBAL_LAYOUT_VERSION, PNPM_VERSION,
    available_parallelism, default_cache_dir, default_config_dir, default_git_shallow_hosts,
    default_peers_suffix_max_length, default_pnpm_home_dir, default_registry, default_state_dir,
    default_unsafe_perm, default_virtual_store_dir_max_length, default_workspace_concurrency,
    install_command_for, is_unsafe_perm_posix, resolve_child_concurrency,
    resolve_configured_state_dir, standalone_install_command,
};
use crate::defaults::{
    default_child_concurrency, default_enable_global_virtual_store, default_fetch_min_speed_ki_bps,
    default_fetch_retries, default_fetch_retry_factor, default_fetch_retry_maxtimeout,
    default_fetch_retry_mintimeout, default_fetch_timeout, default_fetch_warn_timeout_ms,
    default_hoist_pattern, default_modules_cache_max_age, default_modules_dir,
    default_public_hoist_pattern, default_store_dir, default_user_agent, default_virtual_store_dir,
};
pub use workspace_yaml::{
    AllowBuild, AuditSettings, GLOBAL_CONFIG_YAML_FILENAME, LoadWorkspaceYamlError,
    PackageExtension, PeerDependencyMeta, PeerDependencyRules, PnpmfileSetting,
    RemoteSideEffectsCacheSettings, TaskSettings, UpdateConfig, UpdateSettings,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceKeyIssues, WorkspaceSettings, decided_allow_builds,
    registries::{self, RegistryDeclaration, RegistryEntry, RegistryLookups},
    workspace_root_or,
};

impl Config {
    /// The environment is the last word on the remote side-effects cache: it is
    /// where a CI runner injects the signing material that must not be
    /// committed, and where a build job flips publication on for one
    /// invocation.
    ///
    /// Read here rather than by the installer so the values reach it as
    /// ordinary settings. A malformed JSON variable is dropped with a warning
    /// rather than failing the install, matching how the feature degrades to a
    /// local build on every other cache failure.
    pub(crate) fn apply_remote_side_effects_cache_env<Sys: EnvVar>(&mut self) {
        let mut settings = RemoteSideEffectsCacheSettings::default();
        let mut set_any = false;
        if let Some((publish, _)) = side_effects_cache_remote_env::<Sys>("PUBLISH") {
            settings.publish = Some(publish == "true");
            set_any = true;
        }
        for (field, suffix) in [
            (&mut settings.key_id, "KEY_ID"),
            (&mut settings.builder_id, "BUILDER_ID"),
            (&mut settings.image_digest, "IMAGE_DIGEST"),
            (&mut settings.architecture_baseline, "ARCHITECTURE_BASELINE"),
            (&mut settings.private_key, "PRIVATE_KEY"),
        ] {
            if let Some((value, _)) = side_effects_cache_remote_env::<Sys>(suffix) {
                *field = Some(value);
                set_any = true;
            }
        }
        for (field, suffix) in
            [(&mut settings.build_env, "BUILD_ENV"), (&mut settings.trusted_keys, "TRUSTED_KEYS")]
        {
            let Some((value, variable)) = side_effects_cache_remote_env::<Sys>(suffix) else {
                continue;
            };
            match serde_json::from_str::<BTreeMap<String, String>>(&value) {
                Ok(parsed) => {
                    *field = Some(parsed);
                    set_any = true;
                }
                Err(error) => tracing::warn!(
                    target: "pacquet::config",
                    variable,
                    %error,
                    "remote side-effects environment variable is not a string-valued JSON object",
                ),
            }
        }
        if set_any {
            self.remote_side_effects_cache.get_or_insert_default().overlay(settings);
        }
    }
}

fn default_ci<Sys: EnvVar>(detect_ci: fn() -> bool) -> bool {
    let ci = Sys::var("CI");
    if ci.as_deref() == Some("false") {
        return false;
    }

    matches!(ci.as_deref(), Some("true" | "1" | "woodpecker"))
        || Sys::var("GITHUB_ACTIONS").is_some()
        || detect_ci()
}

/// Controls ANSI color rendering in CLI output.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorMode {
    Always,
    #[default]
    Auto,
    Never,
}

impl<'de> Deserialize<'de> for ColorMode {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Bool(bool),
            Mode(Mode),
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        enum Mode {
            Always,
            Auto,
            Never,
        }

        Ok(match Value::deserialize(deserializer)? {
            Value::Bool(true) | Value::Mode(Mode::Always) => ColorMode::Always,
            Value::Bool(false) | Value::Mode(Mode::Never) => ColorMode::Never,
            Value::Mode(Mode::Auto) => ColorMode::Auto,
        })
    }
}

/// `virtualStoreType`: where the virtual store lives, and therefore who
/// shares it.
///
/// Orthogonal to [`NodeLinker`], which picks how a project consumes the
/// store: `pnp` and `isolated` both work with either type, and `hoisted`
/// writes no virtual store at all, so the setting is inert there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VirtualStoreType {
    /// One store per machine, under `<store-dir>/links`. Slots are keyed
    /// by a dependency-graph hash, so every project resolving a package
    /// the same way links to one directory.
    Global,

    /// One store per project, at `<project>/node_modules/.pnpm`. Slots
    /// are keyed by the flat `<name>@<version>` form.
    Project,
}

impl VirtualStoreType {
    /// The `enableGlobalVirtualStore` spelling of this setting.
    #[must_use]
    pub fn is_global(self) -> bool {
        matches!(self, VirtualStoreType::Global)
    }

    /// The type a given `enableGlobalVirtualStore` value selects.
    #[must_use]
    pub fn from_enable_global(enable_global: bool) -> Self {
        if enable_global { VirtualStoreType::Global } else { VirtualStoreType::Project }
    }
}

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
/// by `crate::get_hoisting_limits` in `pnpm-package-manager`.
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
/// `pnpm-resolving-npm-resolver` verifier: under
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

/// The resolved per-package policy in `globalShims`.
///
/// `Auto` (the record value `"auto"`, or its shorthand `true`) defers
/// to artifact authentication:
/// publisher-signature-verified candidates run without prompting, all
/// others go through the candidate-bound trust prompt. `Prompt` always
/// asks, even for authenticated candidates. `Always` always switches
/// without asking — the user pre-answered the prompt in machine-local
/// configuration, which a project cannot write to. `Off` (the record
/// value `false`) disables the package's context-aware shim entirely.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ShimPolicy {
    #[default]
    Off,
    Auto,
    Prompt,
    Always,
}

/// One value of the `globalShims` record: a named policy (`"auto"`,
/// `"prompt"`, `"always"`) or the boolean shorthands (`true` ≡
/// `"auto"`, `false` ≡ disabled). See [`ShimPolicy`] for the semantics
/// each maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShimPolicyValue {
    Toggle(bool),
    Named(NamedShimPolicy),
}

impl ShimPolicy {
    /// The `globalShims` value this policy is written as.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ShimPolicy::Off => "off",
            ShimPolicy::Auto => "auto",
            ShimPolicy::Prompt => "prompt",
            ShimPolicy::Always => "always",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NamedShimPolicy {
    Auto,
    Prompt,
    Always,
}

impl ShimPolicyValue {
    /// Whether a package recorded with this value dispatches at all.
    ///
    /// A recorded value that does not is the user switching a shim off —
    /// including one of the built-in defaults, which is the only way to
    /// switch those off. Clearing such an entry turns the shim back on.
    #[must_use]
    pub fn dispatches(self) -> bool {
        self.resolve() != ShimPolicy::Off
    }

    fn resolve(self) -> ShimPolicy {
        match self {
            ShimPolicyValue::Toggle(false) => ShimPolicy::Off,
            ShimPolicyValue::Toggle(true) | ShimPolicyValue::Named(NamedShimPolicy::Auto) => {
                ShimPolicy::Auto
            }
            ShimPolicyValue::Named(NamedShimPolicy::Prompt) => ShimPolicy::Prompt,
            ShimPolicyValue::Named(NamedShimPolicy::Always) => ShimPolicy::Always,
        }
    }
}

/// One configuration layer of the `globalShims` setting:
/// either a record of package names to policy values, or a scalar
/// shorthand. Layers fold into the resolved [`GlobalShims`]
/// via [`GlobalShims::apply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GlobalShimsSetting {
    /// `globalShims: false` disables every context-aware
    /// shim; `true` resets to the built-in defaults.
    Toggle(bool),
    /// `globalShims: { <package>: <policy> }` merges
    /// key-wise over the defaults and lower layers, so one `bun: false`
    /// entry disables a single default without restating the rest.
    Entries(std::collections::HashMap<String, ShimPolicyValue>),
}

/// The resolved `globalShims` setting: which globally
/// installed packages get context-aware shims and under which trust
/// policy, keyed by the providing package's manifest name (so an entry
/// for `typescript` covers its `tsc` bin).
///
/// The built-in default enables the managed runtimes — `node`, `deno`,
/// and `bun` — with the [`ShimPolicy::Auto`] policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShims {
    entries: std::collections::HashMap<String, ShimPolicy>,
}

impl Default for GlobalShims {
    fn default() -> Self {
        Self {
            entries: ["node", "deno", "bun"]
                .into_iter()
                .map(|name| (name.to_string(), ShimPolicy::Auto))
                .collect(),
        }
    }
}

impl GlobalShims {
    /// Fold one configuration layer into the resolved setting. Records
    /// merge key-wise; the scalar shorthands replace the accumulated
    /// state (`false` → nothing dispatches, `true` → the defaults).
    pub fn apply(&mut self, layer: &GlobalShimsSetting) {
        match layer {
            GlobalShimsSetting::Toggle(false) => self.entries.clear(),
            GlobalShimsSetting::Toggle(true) => *self = Self::default(),
            GlobalShimsSetting::Entries(entries) => {
                for (name, value) in entries {
                    self.entries.insert(name.clone(), value.resolve());
                }
            }
        }
    }

    /// Every package with an entry, and the policy it resolved to.
    pub fn entries(&self) -> impl Iterator<Item = (&str, ShimPolicy)> {
        self.entries.iter().map(|(name, policy)| (name.as_str(), *policy))
    }

    #[must_use]
    pub fn policy(&self, package_name: &str) -> ShimPolicy {
        self.entries.get(package_name).copied().unwrap_or(ShimPolicy::Off)
    }

    #[must_use]
    pub fn is_enabled(&self, package_name: &str) -> bool {
        self.policy(package_name) != ShimPolicy::Off
    }

    /// Whether no package is eligible at all — the dispatcher's cheap
    /// early exit.
    #[must_use]
    pub fn dispatches_nothing(&self) -> bool {
        self.entries.values().all(|policy| *policy == ShimPolicy::Off)
    }
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

impl PmOnFail {
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

/// The module system `pnpm init` records for the package it scaffolds.
///
/// `module` writes `"type": "module"`; `commonjs` is Node's default and
/// leaves the field out of the manifest.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InitType {
    #[default]
    Module,
    Commonjs,
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

/// Tri-state mirror of `pnpm_executor::ScriptsPrependNodePath`
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

/// `saveWorkspaceProtocol`. How a dependency linked to a workspace
/// package is written back to `package.json`.
///
/// The setting is `saveWorkspaceProtocol: boolean | 'rolling'`. Default
/// is [`SaveWorkspaceProtocol::Rolling`]
/// (`'save-workspace-protocol': 'rolling'`).
///
/// [`SaveWorkspaceProtocol::Off`] only suppresses the `workspace:`
/// prefix for a dependency that did not already declare one; a
/// `workspace:` specifier always keeps its protocol, so the two
/// non-rolling states behave alike wherever the protocol is already
/// present.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SaveWorkspaceProtocol {
    /// `false`.
    Off,
    /// `true`. The resolved version is written under the protocol,
    /// keeping the range operator the dependency already declared
    /// (`workspace:^1.2.3`).
    On,
    /// `"rolling"`. The range operator is written without a version
    /// (`workspace:*`, `workspace:^`, `workspace:~`), so the entry
    /// never needs rewriting when the workspace package's version
    /// changes.
    #[default]
    Rolling,
}

impl serde::Serialize for SaveWorkspaceProtocol {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        match self {
            SaveWorkspaceProtocol::Off => serializer.serialize_bool(false),
            SaveWorkspaceProtocol::On => serializer.serialize_bool(true),
            SaveWorkspaceProtocol::Rolling => serializer.serialize_str("rolling"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for SaveWorkspaceProtocol {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct V;
        impl Visitor<'_> for V {
            type Value = SaveWorkspaceProtocol;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a boolean or the string "rolling""#)
            }
            fn visit_bool<DeError: de::Error>(self, value: bool) -> Result<Self::Value, DeError> {
                Ok(if value { SaveWorkspaceProtocol::On } else { SaveWorkspaceProtocol::Off })
            }
            fn visit_str<DeError: de::Error>(self, value: &str) -> Result<Self::Value, DeError> {
                match value {
                    "rolling" => Ok(SaveWorkspaceProtocol::Rolling),
                    other => Err(DeError::invalid_value(
                        de::Unexpected::Str(other),
                        &r#"true, false, or "rolling""#,
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
    /// Try the platform's cheap link tiers in order — hardlink first on
    /// Linux, clone first elsewhere — and fall back to copying when none
    /// is possible. `deps-restorer::link_file::next_auto_tier` implements
    /// the ladder and carries the rationale.
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
/// file directly. Yaml is parsed into [`WorkspaceSettings`] and applied
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

    /// Per-request network timeout in milliseconds. The `fetchTimeout`
    /// setting (default `60000` — 60 s, see
    /// [`pnpm_network::DEFAULT_FETCH_TIMEOUT_MS`]). Applied as both
    /// the response and connect deadline of the reqwest client.
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

    /// When `true`, `add` / `remove` / `update` prune
    /// [`Self::minimum_release_age_exclude`] entries in
    /// `pnpm-workspace.yaml` whose versions the freshly resolved
    /// lockfile no longer records, once the install has written that
    /// lockfile. The `minimumReleaseAgeExcludePrune` setting;
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
    /// [`AuditSettings::ignore_prune`].
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
    pub proxy: pnpm_network::ProxyConfig,
    /// The trusted layers' merged proxy keys, of which [`Self::proxy`]
    /// is the resolution — see [`crate::proxy_keys`].
    pub proxy_keys: crate::proxy_keys::ProxyKeys,
    pub tls: pnpm_network::TlsConfig,
    pub tls_by_uri: pnpm_network::PerRegistryTls,
    pub auth_headers: std::sync::Arc<pnpm_network::AuthHeaders>,
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

    /// The resolved settings used to construct an install HTTP client.
    #[must_use]
    pub fn network_settings(&self) -> pnpm_network::NetworkSettings {
        pnpm_network::NetworkSettings {
            network_concurrency: self.network_concurrency,
            fetch_timeout: std::time::Duration::from_millis(self.fetch_timeout),
            fetch_warn_timeout: std::time::Duration::from_millis(self.fetch_warn_timeout_ms),
            fetch_min_speed_ki_bps: self.fetch_min_speed_ki_bps,
            user_agent: self.user_agent.clone(),
        }
    }

    /// The registries this config declares, in the shape the `registries`
    /// setting is written in — what a pnpr server is told about them.
    ///
    /// The default registry is not among them: it travels as the request's
    /// own `registry` field.
    #[must_use]
    pub fn registry_declarations(&self) -> BTreeMap<String, RegistryDeclaration> {
        registries::to_declarations(&self.registry_lookups(None))
    }

    /// The registries this config resolves from, merged across every source,
    /// in the shape the `registries` setting is written in — the view
    /// `pnpm config get registries` prints. Unlike
    /// [`Self::registry_declarations`], nothing is omitted: the default
    /// registry is declared as the bare `@` scope, and the built-in routes —
    /// the `@jsr` scope and the [`BUILTIN_REGISTRIES_BY_PREFIX`] prefixes —
    /// are declared too, unless the user pointed them elsewhere.
    #[must_use]
    pub fn resolved_registry_declarations(&self) -> BTreeMap<String, RegistryDeclaration> {
        let mut lookups = self.registry_lookups(Some(self.registry.clone()));
        lookups
            .registries_by_scope
            .entry("@jsr".to_string())
            .or_insert_with(|| DEFAULT_JSR_REGISTRY.to_string());
        for (prefix, registry) in BUILTIN_REGISTRIES_BY_PREFIX {
            lookups
                .registries_by_prefix
                .entry((*prefix).to_string())
                .or_insert_with(|| (*registry).to_string());
        }
        registries::to_resolved_declarations(&lookups)
    }

    fn registry_lookups(&self, default_registry: Option<String>) -> RegistryLookups {
        RegistryLookups {
            registries_by_scope: self.registries_by_scope.clone(),
            default_registry,
            registries_by_prefix: self.registries_by_prefix.clone(),
            registry_options_by_url: self.registry_options_by_url.clone(),
        }
    }

    /// The `update` settings the CLI acts on, re-joined from
    /// [`Self::update_config`] — the view `pnpm config get update` prints.
    /// `None` when nothing is set.
    #[must_use]
    pub fn resolved_update_settings(&self) -> Option<UpdateSettings> {
        let update = UpdateSettings {
            ignore_deps: self.update_config.ignore_dependencies.clone(),
            changeset: self.update_config.changeset,
            github_actions: self.update_config.github_actions,
            github_actions_server: self.update_config.github_actions_server.clone(),
        };
        (update != UpdateSettings::default()).then_some(update)
    }

    /// The `audit` settings the CLI acts on, re-joined from
    /// [`Self::audit_level`] and [`Self::audit_config`] — the view
    /// `pnpm config get audit` prints. An empty ignore list reads as unset.
    /// `None` when nothing is set.
    #[must_use]
    pub fn resolved_audit_settings(&self) -> Option<AuditSettings> {
        let audit = AuditSettings {
            level: self.audit_level,
            ignore: (!self.audit_config.ignore_ghsas.is_empty())
                .then(|| self.audit_config.ignore_ghsas.clone()),
            ignore_prune: self.audit_ignore_prune,
        };
        (audit != AuditSettings::default()).then_some(audit)
    }

    /// Overlay the CLI's proxy flags onto the merged keys and re-resolve.
    ///
    /// Only the empty string reads as unset here. A flag carries its value
    /// verbatim, so it has none of the scalar typing that turns a `false` or
    /// `null` in an `.npmrc` or yaml into a non-string; on the command line
    /// those are ordinary hostnames.
    pub fn apply_proxy_cli_overrides(
        &mut self,
        https_proxy: Option<&str>,
        http_proxy: Option<&str>,
        no_proxy: Option<&str>,
    ) {
        for (proxy, keys) in [
            (&mut self.proxy, &mut self.proxy_keys),
            (
                &mut self.package_manager_bootstrap.proxy,
                &mut self.package_manager_bootstrap.proxy_keys,
            ),
        ] {
            for (key, raw) in [
                (&mut keys.https_proxy, https_proxy),
                (&mut keys.http_proxy, http_proxy),
                (&mut keys.no_proxy, no_proxy),
            ] {
                if let Some(raw) = raw {
                    *key = crate::proxy_keys::ProxyValue::from_flag(raw);
                }
            }
            *proxy = keys.resolve();
        }
    }

    /// Effective value of [`Self::minimum_release_age_strict`]: the
    /// user-supplied value when set, otherwise `true` if `minimumReleaseAge`
    /// was explicitly configured.
    ///
    /// Without that default a user-set cutoff would silently fall back to an
    /// immature version whenever no mature one satisfies the range, making the
    /// setting look like it had no effect. The built-in 1440-minute default
    /// stays non-strict for backward compatibility, so the two are told apart
    /// through [`Self::explicit_settings`] rather than through the value
    /// itself. A repository's cutoff never reaches `self-update`, which
    /// [`WorkspaceSettings::clear_self_update_policy`] drops before the
    /// workspace yaml is recorded there.
    ///
    /// [`WorkspaceSettings::clear_self_update_policy`]: crate::WorkspaceSettings::clear_self_update_policy
    pub fn resolved_minimum_release_age_strict(&self) -> bool {
        self.minimum_release_age_strict
            .unwrap_or_else(|| self.explicit_settings.contains_key("minimumReleaseAge"))
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
        self.full_metadata_policy(self.registry_supports_time_field)
    }

    /// The same policy, asked of one registry: a registry that declares
    /// `supportsTimeField` answers for itself, so a time-based resolution
    /// reads abbreviated metadata from the registries that carry `time` and
    /// full metadata only from the ones that do not.
    ///
    /// A registry with no declaration answers exactly what
    /// [`Self::requires_full_metadata_for_resolution`] does.
    #[must_use]
    pub fn requires_full_metadata_for_registry(&self, registry: &str) -> bool {
        self.full_metadata_policy(self.registry_supports_time_field(registry))
    }

    /// Whether a full packument, once fetched, is stored and read in pnpm's
    /// filtered form rather than verbatim.
    ///
    /// Answered for the most demanding registry — one that carries no `time`
    /// — because [`Self::requires_full_metadata_for_registry`] can ask for a
    /// full document at a registry
    /// [`Self::requires_full_metadata_for_resolution`] would have left on
    /// abbreviated metadata, and both have to agree on which mirror that
    /// document lands in. It is consulted only when a full document is
    /// actually fetched, so answering for the demanding case costs the others
    /// nothing.
    #[must_use]
    pub fn requires_filtered_full_metadata(&self) -> bool {
        self.full_metadata_policy(false)
    }

    /// Whether `registry`'s abbreviated metadata carries the `time` field,
    /// from its own declaration if it has one and from the
    /// `registrySupportsTimeField` setting otherwise.
    #[must_use]
    pub fn registry_supports_time_field(&self, registry: &str) -> bool {
        pnpm_lockfile::registry_supports_time_field(&self.registry_options_by_url, registry)
            .unwrap_or(self.registry_supports_time_field)
    }

    fn full_metadata_policy(&self, supports_time_field: bool) -> bool {
        full_metadata_policy(
            self.trust_policy,
            self.resolution_mode == ResolutionMode::TimeBased,
            supports_time_field,
        )
    }

    /// [`Self::requires_full_metadata_for_registry`] as a closure the resolver
    /// can hold, capturing the four facts it needs rather than the config.
    #[must_use]
    pub fn requires_full_metadata_for_registry_fn(&self) -> NeedsFullMetadataFor {
        let registry_options_by_url = self.registry_options_by_url.clone();
        let default_supports_time_field = self.registry_supports_time_field;
        let trust_policy = self.trust_policy;
        let time_based = self.resolution_mode == ResolutionMode::TimeBased;
        Arc::new(move |registry: &str| {
            let supports_time_field =
                pnpm_lockfile::registry_supports_time_field(&registry_options_by_url, registry)
                    .unwrap_or(default_supports_time_field);
            full_metadata_policy(trust_policy, time_based, supports_time_field)
        })
    }

    /// Registry map in pnpm's `Registries` shape: `default` plus the
    /// configured scoped routes keyed by `@scope`.
    #[must_use]
    pub fn resolved_registries(&self) -> BTreeMap<String, String> {
        let mut registries = self.registries_by_scope.clone();
        registries.insert("default".to_string(), self.registry.clone());
        registries
    }

    /// Apply a boolean `sideEffectsCache` declaration, which turns the
    /// local read and write gates on or off together.
    ///
    /// [`Config::side_effects_cache_read`] and
    /// [`Config::side_effects_cache_write`] prefer the object form's
    /// fields, so a layer spelling the setting as a boolean has to clear
    /// what an earlier layer's object left behind to beat it. The remote
    /// tier is a separate declaration the boolean says nothing about, so
    /// it survives untouched.
    pub fn apply_side_effects_cache_shorthand(&mut self, enabled: bool) {
        self.side_effects_cache = enabled;
        self.side_effects_cache_read_setting = None;
        self.side_effects_cache_write_setting = None;
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
        self.side_effects_cache_read_setting
            .unwrap_or(self.side_effects_cache || self.side_effects_cache_readonly)
    }

    /// Whether the install is allowed to populate the side-effects
    /// cache after a successful postinstall
    /// (`sideEffectsCacheWrite = sideEffectsCache`), with the additional
    /// constraint that the explicit `sideEffectsCacheReadonly: true`
    /// always wins — a `??` would let `readonly` slip through when both
    /// flags are explicitly set, but `readonly` as a flag name only makes
    /// sense if it really does block writes.
    pub fn side_effects_cache_write(&self) -> bool {
        self.side_effects_cache_write_setting
            .unwrap_or(self.side_effects_cache && !self.side_effects_cache_readonly)
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
    ///   (or, in practice, through `pnpm_package_manager::VirtualStoreLayout`).
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

    /// The directory owning the `pnpm-lock.yaml` that covers
    /// `project_dir`: the pinned [`lockfile_dir`], else the workspace root
    /// when the workspace shares one lockfile, else the project itself.
    ///
    /// Mirrors pnpm's `lockfileDir ?? dir`, whose config reader has
    /// already defaulted `lockfileDir` to `workspaceDir` for a shared
    /// workspace lockfile.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    #[must_use]
    pub fn lockfile_dir_for<'a>(&'a self, project_dir: &'a Path) -> &'a Path {
        self.lockfile_dir.as_deref().unwrap_or_else(|| {
            if self.shared_workspace_lockfile {
                self.workspace_dir.as_deref().unwrap_or(project_dir)
            } else {
                project_dir
            }
        })
    }

    /// Whether one `pnpm-lock.yaml` covers every project the command
    /// touches. The `sharedWorkspaceLockfile` setting, which an explicit
    /// [`lockfile_dir`] overrides: pinning the lockfile to one directory
    /// *is* the shared layout, and pnpm's recursive dispatch routes such
    /// a run through its shared-lockfile branch whatever the setting
    /// says.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    #[must_use]
    pub fn shares_one_lockfile(&self) -> bool {
        self.lockfile_dir.is_some() || self.shared_workspace_lockfile
    }

    /// pnpm's `rootProjectManifestDir`: where the root `package.json`,
    /// the config dependencies (`node_modules/.pnpm-config`), and the
    /// pnpmfile a command reads live — `lockfileDir ?? workspaceDir ??
    /// dir`.
    ///
    /// Not the directory settings are *written* back to: `pnpm-workspace.yaml`
    /// stays at [`workspace_dir`] when there is one.
    ///
    /// [`workspace_dir`]: Self::workspace_dir
    #[must_use]
    pub fn root_project_manifest_dir<'a>(&'a self, dir: &'a Path) -> &'a Path {
        self.lockfile_dir.as_deref().or(self.workspace_dir.as_deref()).unwrap_or(dir)
    }

    /// Pin [`lockfile_dir`] to `dir` and re-anchor the paths that follow
    /// the lockfile with it.
    ///
    /// `dir` is normalized first: importer ids are a lexical path diff
    /// against it, so an unnormalized `<workspace>/..` would not name the
    /// project it points at.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    pub fn pin_lockfile_dir(&mut self, dir: &Path) {
        let dir = pnpm_fs::lexical_normalize(dir);
        self.anchor_lockfile_paths(&dir);
        self.lockfile_dir = Some(dir);
    }

    /// Re-anchor the paths pnpm resolves against `lockfileDir` — the root
    /// `node_modules` and the virtual store — onto `dir`.
    ///
    /// An explicitly configured `modulesDir` / `virtualStoreDir` keeps its
    /// raw value (recovered from [`explicit_settings`]) and is re-resolved
    /// against `dir`, so a multi-component or absolute setting keeps its
    /// full shape — [`Path::join`] leaves an absolute value absolute.
    /// Global-virtual-store installs keep their store-anchored
    /// `virtual_store_dir`.
    ///
    /// [`explicit_settings`]: Self::explicit_settings
    pub fn anchor_lockfile_paths(&mut self, dir: &Path) {
        self.modules_dir =
            match self.explicit_settings.get("modulesDir").and_then(serde_json::Value::as_str) {
                Some(raw) => dir.join(raw),
                None => dir.join("node_modules"),
            };
        if !self.enable_global_virtual_store {
            self.virtual_store_dir = match self
                .explicit_settings
                .get("virtualStoreDir")
                .and_then(serde_json::Value::as_str)
            {
                Some(raw) => dir.join(raw),
                None => self.modules_dir.join(".pnpm"),
            };
        }
    }

    /// [`Config::extra_env`] with the `nodeOptions` setting applied as
    /// `NODE_OPTIONS`, preserving the ESM `NODE_PATH` loader flag the
    /// `extra_env` carries under a global virtual store.
    pub fn extra_env_with_node_options(&self) -> HashMap<String, String> {
        let mut extra_env = self.extra_env.clone();
        if let Some(node_options) = &self.node_options {
            let node_options = esm_node_path_loader::keep_esm_node_path_loader_option(
                node_options,
                self.extra_env.get("NODE_OPTIONS").map(String::as_str),
            );
            extra_env.insert("NODE_OPTIONS".to_string(), node_options);
        }
        extra_env
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
        if self.hoist_patterns_before_virtual_store_only.is_none() {
            self.hoist_patterns_before_virtual_store_only = Some(HoistPatterns {
                hoist_pattern: self.hoist_pattern.take(),
                public_hoist_pattern: self.public_hoist_pattern.take(),
            });
        }
        self.hoist_pattern = Some(Vec::new());
        self.public_hoist_pattern = Some(Vec::new());
    }

    /// Undo [`apply_virtual_store_only_derivation`] after a command-line
    /// `--no-virtual-store-only` outranks a lower layer's
    /// `virtualStoreOnly: true`, which emptied both patterns when the
    /// config was built. pnpm merges the command line before it derives,
    /// so it never empties them in the first place.
    ///
    /// [`apply_virtual_store_only_derivation`]: Self::apply_virtual_store_only_derivation
    pub fn restore_hoist_patterns_after_virtual_store_only(&mut self) {
        if self.virtual_store_only {
            return;
        }
        if let Some(patterns) = self.hoist_patterns_before_virtual_store_only.take() {
            self.hoist_pattern = patterns.hoist_pattern;
            self.public_hoist_pattern = patterns.public_hoist_pattern;
        }
    }

    /// The lockfile file name this install reads first and writes back:
    /// the branch lockfile under `gitBranchLockfile`, `pnpm-lock.yaml`
    /// otherwise.
    ///
    /// `mergeGitBranchLockfiles` wins over the branch name — the point of
    /// that mode is to collapse the per-branch lockfiles back into the
    /// shared one.
    #[must_use]
    pub fn wanted_lockfile_name(&self) -> &str {
        match &self.git_branch_lockfile_name {
            Some(name) if !self.merge_git_branch_lockfiles => name,
            _ => Lockfile::FILE_NAME,
        }
    }

    /// [`Self::wanted_lockfile_name`] paired with the merge flag, as the
    /// lockfile loader wants them.
    #[must_use]
    pub fn wanted_lockfile_selection(&self) -> WantedLockfileSelection {
        WantedLockfileSelection {
            file_name: self.wanted_lockfile_name().to_owned(),
            merge_git_branch_lockfiles: self.merge_git_branch_lockfiles,
        }
    }

    /// Resolve the per-branch lockfile settings against the git branch the
    /// process is on: which `pnpm-lock.<branch>.yaml` an install under
    /// `gitBranchLockfile` uses, and whether
    /// `mergeGitBranchLockfilesBranchPattern` puts this branch in merge
    /// mode.
    ///
    /// The branch is read from the process's working directory, which is
    /// where pnpm reads it from too — not from the workspace root, which
    /// may sit in a different repository than the one the user is in.
    pub fn apply_git_branch_lockfile_derivation<Sys: GetCurrentDir>(&mut self) {
        // An explicit `mergeGitBranchLockfiles` — including an explicit
        // `false` — settles the question without consulting the pattern.
        let merge_is_explicit = self.explicit_settings.contains_key("mergeGitBranchLockfiles");
        let pattern_decides =
            !merge_is_explicit && !self.merge_git_branch_lockfiles_branch_pattern.is_empty();
        if !self.use_git_branch_lockfile && !pattern_decides {
            return;
        }
        let Ok(cwd) = Sys::current_dir() else { return };
        let Some(branch) = get_current_branch::<GitHost>(&cwd) else { return };
        if pattern_decides {
            self.merge_git_branch_lockfiles =
                create_matcher(&self.merge_git_branch_lockfiles_branch_pattern).matches(&branch);
        }
        if self.use_git_branch_lockfile {
            self.git_branch_lockfile_name = Some(Lockfile::git_branch_file_name(&branch));
        }
    }

    /// Apply the legacy `shamefullyHoist` setting to the public hoist pattern.
    ///
    /// This runs after all config sources have been merged because an explicit
    /// `shamefullyHoist` value takes precedence over `publicHoistPattern`
    /// regardless of which source supplied either setting.
    pub fn apply_shamefully_hoist_derivation(&mut self) {
        match self.explicit_settings.get("shamefullyHoist").and_then(serde_json::Value::as_bool) {
            Some(true) => self.public_hoist_pattern = Some(vec!["*".to_string()]),
            Some(false) => self.public_hoist_pattern = None,
            None => {}
        }
    }

    /// Turn [`prefer_symlinked_executables`] on when the hoisted
    /// `nodeLinker` is selected and the user has not configured the
    /// setting — pnpm's `nodeLinker: hoisted` default. Runs *after* the
    /// `NODE_PATH` export in [`Config::current`], so the derived `true`
    /// symlinks bins without exporting `NODE_PATH` (the hoisted layout
    /// has no hidden store to expose), exactly like pnpm's config
    /// reader. Also re-applied by the CLI's `--config.node-linker`
    /// override, which lands after [`Config::current`] has run.
    ///
    /// A user-configured value — recorded in `explicit_settings` by
    /// every config layer — is never touched. Otherwise the derived
    /// value tracks the *current* linker, so re-running after a linker
    /// override also clears a `true` derived for a linker that is no
    /// longer selected (pnpm merges CLI options before its `nodeLinker`
    /// switch, so its derivation only ever sees the final linker).
    ///
    /// [`prefer_symlinked_executables`]: Self::prefer_symlinked_executables
    pub fn apply_prefer_symlinked_executables_derivation(&mut self) {
        if self.explicit_settings.contains_key("preferSymlinkedExecutables") {
            return;
        }
        self.prefer_symlinked_executables =
            (self.node_linker == NodeLinker::Hoisted).then_some(true);
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

    /// Resolve the default store location relative to an explicit pnpm home
    /// directory instead of the ambient one — the programmatic counterpart
    /// of the `pnpmHomeDir` input of pnpm's `getStorePath`. The store lands
    /// at `<pnpm_home_dir>/store/<version>` when `start_dir` can hardlink
    /// into that volume, with the same mount-point fallback as the ambient
    /// default. Callers apply it only when no config source set `storeDir`.
    pub fn resolve_store_dir_from_home<Sys>(&mut self, pnpm_home_dir: &Path, start_dir: &Path)
    where
        Sys: GetHomeDir + LinkProbe,
    {
        self.store_dir = StoreDir::new(pnpm_home_dir.join("store"));
        self.resolve_default_store_dir::<Sys>(start_dir);
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
        let default_state_dir = default_state_dir::<Sys>().unwrap_or_default();
        self.state_dir.clone_from(&default_state_dir);

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
        //
        // `--ignore-workspace` stops the search outright, which is what
        // makes the flag mean "standalone project": with no workspace dir
        // there is no shared lockfile, no sibling projects, and no
        // `pnpm-workspace.yaml` settings layer. Only the flag reaches
        // this far — see [`Config::ignore_workspace`].
        let env_workspace_dir = Sys::var_os("NPM_CONFIG_WORKSPACE_DIR")
            .or_else(|| Sys::var_os("npm_config_workspace_dir"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let workspace_yaml = if self.ignore_workspace {
            None
        } else if let Some(env_dir) = env_workspace_dir {
            // Env-var path: load yaml directly from the env dir. A
            // missing file is silent, but the re-anchor still fires
            // because the user has explicitly told us where the
            // workspace lives.
            let yaml_path = env_dir.join(WORKSPACE_MANIFEST_FILENAME);
            match fs::read_to_string(&yaml_path) {
                Ok(text) => {
                    let mut settings: WorkspaceSettings =
                        serde_saphyr::from_str(&text).map_err(Box::new).map_err(|source| {
                            LoadWorkspaceYamlError::ParseYaml { path: yaml_path, source }
                        })?;
                    settings.collect_key_issues(&text);
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
        // The rescope warning names the file it read, so each source
        // labels itself with the path it was actually loaded from.
        let parse_trusted_source = |text: String, dir: PathBuf, path: &Path| {
            let mut auth = NpmrcAuth::from_ini::<Sys>(&text, &dir);
            auth.rescope_unscoped(&path.display().to_string());
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
            auth.rescope_unscoped(&project_npmrc_path.display().to_string());
            auth
        });
        let auth_ini_source = global_config_dir.as_deref().and_then(|dir| {
            let path = dir.join("auth.ini");
            read_npmrc_file(&path).map(|text| parse_trusted_source(text, dir.to_path_buf(), &path))
        });
        let user_source = match &user_npmrc_path {
            Some(path) => read_npmrc_file(path).map(|text| {
                // Relative `cafile`/`certfile` entries resolve against
                // the file's directory; for a bare filename (no parent)
                // that's the empty path — i.e. the process cwd — never
                // the file itself.
                let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                parse_trusted_source(text, dir, path)
            }),
            None => Sys::home_dir().and_then(|dir| {
                let path = dir.join(".npmrc");
                read_npmrc(&dir).map(|text| parse_trusted_source(text, dir, &path))
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
            let bootstrap = &mut self.package_manager_bootstrap;
            global_settings.apply_proxy_to(&mut bootstrap.proxy, &mut bootstrap.proxy_keys);
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
        // Path-valued fields other than `stateDir` use `start_dir` as the
        // base for relative resolution — pnpm passes `workspaceDir:
        // undefined` for the global manifest, which leaves paths
        // un-anchored. Using `start_dir` here is a small pacquet-specific
        // extension that keeps relative paths well-defined; users putting
        // absolute paths (the recommended pattern) see no difference.
        // `stateDir` goes through [`resolve_configured_state_dir`] because
        // it carries global-shim trust records and must not resolve under
        // the project being considered for execution.
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
        // Collected as each file is applied, since applying it is what makes
        // a declared route indistinguishable by value from a resolved one.
        let mut declared_registries = crate::npmrc_auth::DeclaredRegistries::default();
        if let Some(mut global_settings) = global_settings {
            note_declared_registries(&mut declared_registries, &global_settings);
            virtual_store_dir_explicit |= global_settings.virtual_store_dir.is_some();
            global_virtual_store_dir_explicit |= global_settings.global_virtual_store_dir.is_some();
            store_dir_explicit |= global_settings.store_dir.is_some();
            collect_explicit_settings(&mut self.explicit_settings, &global_settings);
            let configured_state_dir = global_settings.state_dir.take();
            let saved_workspace_dir = self.workspace_dir.take();
            global_settings.expand_global_dir_home_prefixes::<Sys>();
            global_settings.apply_to(&mut self, start_dir);
            self.workspace_dir = saved_workspace_dir;
            if let Some(configured_state_dir) =
                configured_state_dir.as_deref().filter(|value| !value.is_empty())
            {
                self.state_dir =
                    resolve_configured_state_dir(&default_state_dir, configured_state_dir);
            }
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
            self.workspace_package_patterns = Some(
                settings
                    .as_ref()
                    .and_then(|settings| settings.packages.clone())
                    .unwrap_or_else(|| vec![".".to_string()]),
            );
            if let Some(mut settings) = settings {
                // CI detection is process state. A repository-controlled
                // manifest must not be able to turn it off; trusted global
                // config and PNPM_CONFIG_CI are applied in their own layers.
                settings.ci = None;
                settings.state_dir = None;
                settings.scope = None;
                settings.global_dir = None;
                settings.global_bin_dir = None;
                // `|=` rather than `=` so an `enableGlobalVirtualStore` /
                // `virtualStoreDir` set in the global `config.yaml` still
                // counts as "explicitly set" when the workspace yaml
                // leaves it unset.
                virtual_store_dir_explicit |= settings.virtual_store_dir.is_some();
                global_virtual_store_dir_explicit |= settings.global_virtual_store_dir.is_some();
                store_dir_explicit |= settings.store_dir.is_some();
                settings.substitute_env_untrusted::<Sys>();
                if for_self_update {
                    settings.clear_self_update_policy();
                }
                self.workspace_key_issues = settings.key_issues.clone();
                note_declared_registries(&mut declared_registries, &settings);
                collect_explicit_settings(&mut self.explicit_settings, &settings);
                settings.apply_to(&mut self, &base_dir);
                // `overrides` reaches `Config` only from the workspace
                // yaml (the global config.yaml is stripped of the key,
                // and no `PNPM_CONFIG_*` var carries a map), so the
                // `$dep-name` values it may hold are resolved here,
                // against the workspace root's manifest.
                if let Some(overrides) = self.overrides.as_mut() {
                    crate::override_version_references::resolve_version_references(
                        overrides, &base_dir,
                    )?;
                }
            }
        }

        // Apply `_auth` routes after workspace yaml (so they win over
        // repo-controlled registries) but before `PNPM_CONFIG_*` (so an
        // explicit `pnpm_config_registry` / `--registry` still wins) —
        // pnpm's "CLI > _auth > yaml" precedence.
        npmrc_auth.apply_json_env_registries(&mut self, &declared_registries);

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
        collect_explicit_settings(&mut self.explicit_settings, &env_settings);
        let configured_state_dir = env_settings.state_dir.take();
        let bootstrap = &mut self.package_manager_bootstrap;
        env_settings.apply_proxy_to(&mut bootstrap.proxy, &mut bootstrap.proxy_keys);
        let saved_workspace_dir = self.workspace_dir.clone();
        env_settings.expand_global_dir_home_prefixes::<Sys>();
        env_settings.apply_to(&mut self, start_dir);
        self.workspace_dir = saved_workspace_dir;
        self.apply_remote_side_effects_cache_env::<Sys>();
        if let Some(configured_state_dir) =
            configured_state_dir.as_deref().filter(|value| !value.is_empty())
        {
            self.state_dir = resolve_configured_state_dir(&default_state_dir, configured_state_dir);
        }
        if let Some(registry) = env_registry_override {
            let normalized =
                if registry.ends_with('/') { registry } else { format!("{registry}/") };
            self.registries_by_scope.insert("default".to_string(), normalized.clone());
            self.package_manager_bootstrap.registry.clone_from(&normalized);
            self.package_manager_bootstrap.registries.insert("default".to_string(), normalized);
        }

        if !self.explicit_settings.contains_key("lockfile") {
            self.lockfile = self.package_lock;
        }

        // A pinned `lockfileDir` moves the root `node_modules` and the
        // virtual store with it. Applied after every source has had its
        // say so the anchor uses the final value, and before the
        // global-virtual-store derivation, which may re-point
        // `virtual_store_dir` at the store.
        if let Some(lockfile_dir) = self.lockfile_dir.clone() {
            self.anchor_lockfile_paths(&lockfile_dir);
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

        self.apply_git_branch_lockfile_derivation::<Sys>();
        self.apply_shamefully_hoist_derivation();
        self.apply_virtual_store_only_derivation();

        // Resolve the global install directories:
        // `globalPkgDir = (globalDir ?? <pnpm-home>/global)/v11` and
        // `bin = globalBinDir ?? <pnpm-home>/bin`.
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

        // With `preferSymlinkedExecutables`, `.bin` entries are plain
        // symlinks with no shim to carry a `NODE_PATH` block, so the
        // resolution help moves to the environment: expose the virtual
        // store's hidden `node_modules` to every spawned child process.
        // `virtual_store_dir` is already anchored at the workspace root
        // by the re-anchor above — pnpm builds this from
        // `lockfileDir ?? dir` to the same effect
        // (pnpm/pnpm#13912). Unix only, like pnpm; and only an explicit
        // `true` fires — the hoisted-linker derivation below runs after
        // this block, mirroring pnpm's config-reader ordering.
        if cfg!(unix) && self.prefer_symlinked_executables == Some(true) {
            let hidden_modules_dir =
                pnpm_fs::lexical_normalize(&self.virtual_store_dir.join("node_modules"));
            self.extra_env
                .insert("NODE_PATH".to_string(), hidden_modules_dir.display().to_string());
        }
        self.apply_prefer_symlinked_executables_derivation();

        // With a global virtual store, package directories live outside the
        // project, so Node's upward node_modules walk from their real paths
        // never reaches the project's hoisted node_modules or root
        // node_modules. Expose both through NODE_PATH for every child
        // process pnpm spawns, and register the ESM loader that restores
        // NODE_PATH lookups for ESM imports. Mirrors the pnpm config
        // reader (`pnpm11/config/reader/src/index.ts`).
        if self.enable_global_virtual_store
            && self.extend_node_path
            && self.node_linker == NodeLinker::Isolated
        {
            let path_delimiter = if cfg!(windows) { ';' } else { ':' };
            let mut node_paths: Vec<String> = self
                .extra_env
                .get("NODE_PATH")
                .map(|value| value.split(path_delimiter).map(str::to_string).collect())
                .unwrap_or_default();
            for dir in [self.virtual_store_dir.join("node_modules"), self.modules_dir.clone()] {
                // `virtual_store_dir` is built by joining a multi-segment
                // literal, which keeps `/` separators on Windows; normalize
                // so NODE_PATH carries native separators like the shims do.
                let dir = pnpm_fs::lexical_normalize(&dir).display().to_string();
                if !node_paths.contains(&dir) {
                    node_paths.push(dir);
                }
            }
            let node_paths = node_paths.join(&path_delimiter.to_string());
            self.extra_env.insert("NODE_PATH".to_string(), node_paths);
            self.extra_env.insert(
                "NODE_OPTIONS".to_string(),
                esm_node_path_loader::add_esm_node_path_loader_option(
                    Sys::var("NODE_OPTIONS").as_deref(),
                ),
            );
        }

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
///
/// `virtualStoreType` and `enableGlobalVirtualStore` are two spellings of one
/// setting, so a source that sets either one decides both: the record follows
/// [`WorkspaceSettings::apply_to`] and fills in the spelling the source left
/// out, or `pnpm config get` would answer one of the two with the value the
/// install did not use.
/// Record what `settings` declares about registry routing, before
/// [`WorkspaceSettings::apply_to`] consumes it.
fn note_declared_registries(
    declared: &mut crate::npmrc_auth::DeclaredRegistries,
    settings: &WorkspaceSettings,
) {
    declared.registry |= settings.registry.is_some();
    let Some(entries) = settings.registries.as_ref() else {
        return;
    };
    for scope in crate::workspace_yaml::registries::routed_scopes(entries) {
        if scope == crate::workspace_yaml::registries::DEFAULT_REGISTRY_SCOPE {
            declared.registry = true;
        } else {
            declared.scopes.insert(scope);
        }
    }
}

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
    let virtual_store_type = settings
        .virtual_store_type
        .or_else(|| settings.enable_global_virtual_store.map(VirtualStoreType::from_enable_global));
    if let Some(virtual_store_type) = virtual_store_type {
        let Ok(named) = serde_json::to_value(virtual_store_type) else { return };
        target.insert("virtualStoreType".to_string(), named);
        target.insert(
            "enableGlobalVirtualStore".to_string(),
            serde_json::Value::Bool(virtual_store_type.is_global()),
        );
    }
    // `audit.level` supersedes the deprecated `auditLevel` spelling; mirror it
    // there so `config get audit-level` answers the way pnpm does.
    if let Some(level) = settings.audit.as_ref().and_then(|audit| audit.level) {
        let Ok(level) = serde_json::to_value(level) else { return };
        target.insert("auditLevel".to_string(), level);
    }
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
    let mut config = Config::default();
    trusted_auth.apply_registry_and_warn(&mut config);
    // No config file reaches the bootstrap cascade, so none declares here.
    trusted_auth
        .apply_json_env_registries(&mut config, &crate::npmrc_auth::DeclaredRegistries::default());
    trusted_auth.apply_proxy_cascade::<Sys>(&mut config);
    trusted_auth.apply_tls_and_local_address(&mut config);
    trusted_auth.build_auth_headers(&mut config)?;
    Ok(PackageManagerBootstrap {
        registry: config.registry,
        registries: config.registries_by_scope,
        proxy: config.proxy,
        proxy_keys: config.proxy_keys,
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

/// Whether the resolution has to read full packument metadata from a given
/// registry, as [`Config::requires_full_metadata_for_registry_fn`] answers it.
pub type NeedsFullMetadataFor = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Whether a resolution has to read full packument metadata: trust evidence
/// (`_npmUser`) is never in the abbreviated form, and a time-based resolution
/// needs `time`, which a registry may or may not carry there.
fn full_metadata_policy(
    trust_policy: TrustPolicy,
    time_based: bool,
    supports_time_field: bool,
) -> bool {
    trust_policy == TrustPolicy::NoDowngrade || (time_based && !supports_time_field)
}

/// Reads one field of the remote tier from the environment, under the name
/// that matches the setting and under the one that matched its older spelling.
///
/// A machine configured for `remoteSideEffectsCache` keeps working; a machine
/// setting both gets the name that matches the setting it is configuring.
/// The name comes back with the value because a malformed one is reported by
/// name, and naming a variable the user did not set sends them looking for it.
fn side_effects_cache_remote_env<Sys: EnvVar>(suffix: &str) -> Option<(String, String)> {
    for variable in [
        format!("PNPM_SIDE_EFFECTS_CACHE_REMOTE_{suffix}"),
        format!("PNPM_REMOTE_SIDE_EFFECTS_CACHE_{suffix}"),
    ] {
        if let Some(value) = Sys::var(&variable) {
            return Some((value, variable));
        }
    }
    None
}
