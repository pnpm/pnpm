pub mod config_types;
pub mod esm_node_path_loader;
pub mod known_settings;
pub mod naming_cases;
pub mod property_path;
pub mod protected_settings;
pub mod proxy_keys;
pub mod refused_keys;
pub mod version_policy;
pub use crate::{
    api::{EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe},
    defaults::{
        BUILTIN_REGISTRIES_BY_PREFIX, DEFAULT_JSR_REGISTRY, GLOBAL_LAYOUT_VERSION, PNPM_VERSION,
        available_parallelism, default_cache_dir, default_config_dir, default_git_shallow_hosts,
        default_peers_suffix_max_length, default_pnpm_home_dir, default_registry,
        default_state_dir, default_unsafe_perm, default_virtual_store_dir_max_length,
        default_workspace_concurrency, install_command_for, is_unsafe_perm_posix,
        resolve_child_concurrency, resolve_configured_state_dir, standalone_install_command,
    },
    global_bin_check::{CheckGlobalBinDirError, check_global_bin_dir},
    npmrc_auth::{BasicAuth, RegistryCreds, is_json_auth_scope, validate_json_auth_registry},
};
pub use pnpm_matcher as matcher;
pub use setting_types::{
    AuditConfig, AuditLevel, CatalogMode, ColorMode, HoistingLimits, InitType,
    LinkWorkspacePackages, NodeLinker, NodePackageMapType, PackageImportMethod, PmOnFail,
    ResolutionMode, RuntimeOnFail, SaveWorkspaceProtocol, ScriptsPrependNodePath, TrustPolicy,
    VerifyDepsBeforeRun, VirtualStoreType,
};
pub use settings::{Config, HoistPatterns};
pub use shim_policy::{
    GlobalShims, GlobalShimsSetting, NamedShimPolicy, ShimPolicy, ShimPolicyValue,
};
pub use workspace_yaml::{
    AllowBuild, AuditSettings, CargoSettings, GLOBAL_CONFIG_YAML_FILENAME, LoadWorkspaceYamlError,
    PackageExtension, PeerDependencyMeta, PeerDependencyRules, PnpmfileSetting, PythonSettings,
    RemoteSideEffectsCacheSettings, TaskSettings, UpdateConfig, UpdateSettings,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceKeyIssues, WorkspaceSettings, decided_allow_builds,
    package_configs::{self, PackageConfigsSetting, ProjectConfig, ProjectConfigMultiMatch},
    registries::{self, RegistryDeclaration, RegistryEntry, RegistryLookups},
    workspace_root_or,
};

mod api;
mod defaults;
mod env_overlay;
mod global_bin_check;
mod npmrc_auth;
mod override_version_references;
mod store_path;
mod workspace_yaml;

use crate::{
    defaults::{
        default_child_concurrency, default_enable_global_virtual_store,
        default_fetch_min_speed_ki_bps, default_fetch_retries, default_fetch_retry_factor,
        default_fetch_retry_maxtimeout, default_fetch_retry_mintimeout, default_fetch_timeout,
        default_fetch_warn_timeout_ms, default_hoist_pattern, default_modules_cache_max_age,
        default_modules_dir, default_public_hoist_pattern, default_store_dir, default_user_agent,
        default_virtual_store_dir,
    },
    npmrc_auth::NpmrcAuth,
};
use indexmap::IndexMap;
use pipe_trait::Pipe;
use pnpm_git_utils::{Host as GitHost, get_current_branch};
use pnpm_lockfile::{Lockfile, RegistryOptions, WantedLockfileSelection};
use pnpm_matcher::create_matcher;
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

/// Which path-valued settings the user pinned somewhere in the cascade, as
/// opposed to letting a `SmartDefault` fill them in. The derivations that
/// re-point these paths must not clobber a value the user chose.
#[derive(Debug, Default, Clone, Copy)]
struct ExplicitPaths {
    virtual_store_dir: bool,
    global_virtual_store_dir: bool,
    store_dir: bool,
}

impl ExplicitPaths {
    /// Record what one settings layer pinned.
    fn note(&mut self, settings: &WorkspaceSettings) {
        self.virtual_store_dir |= settings.virtual_store_dir.is_some();
        self.global_virtual_store_dir |= settings.global_virtual_store_dir.is_some();
        self.store_dir |= settings.store_dir.is_some();
    }
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
    /// configured scoped routes.
    ///
    /// The built-in `@jsr` route [`Config::resolved_registries`] carries is
    /// left out: this map resolves the package manager alone, which is never
    /// a JSR package.
    #[must_use]
    pub fn resolved_registries(&self) -> BTreeMap<String, String> {
        let mut registries = self.registries.clone();
        registries.insert("default".to_string(), self.registry.clone());
        registries
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
    // The trusted `.npmrc` files are the only config files that reach the
    // bootstrap cascade, so only what they declare holds the `_auth` file's
    // routes back here.
    let mut declared_registries = crate::npmrc_auth::DeclaredRegistries::default();
    trusted_auth.apply_registry_and_warn(&mut config, &mut declared_registries);
    trusted_auth.apply_json_env_registries(&mut config, &declared_registries);
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

mod shim_policy;

mod setting_types;

mod registry_options;

mod layout;

mod loading;

mod derive_layout;

mod workspace_settings;

mod auth_sources;

mod settings;

use auth_sources::{AuthSources, note_declared_registries};
