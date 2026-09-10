pub use workspace::{
    CatalogMode, LinkWorkspacePackages, PackageImportMethod, ResolutionMode, SaveWorkspaceProtocol,
};

use super::{Deserialize, Serialize};

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

/// What to do when the project's `packageManager` /
/// `devEngines.packageManager` field doesn't match the running pnpm.
///
/// The setting is `'download' | 'error' | 'warn' | 'ignore'`. `download`
/// switches to the pinned version, `error` aborts, `warn` prints a
/// warning, and `ignore` skips the check entirely. The documented
/// default is `download`, so [`Config::pm_on_fail`](crate::settings::Config::pm_on_fail) stays optional and the
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
/// The command-level default is `low`, so [`Config::audit_level`](crate::settings::Config::audit_level) stays
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

mod workspace;
