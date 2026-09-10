use super::{
    AllowBuild, LoadWorkspaceYamlError, RemoteSideEffectsCacheSettings, SideEffectsCacheSetting,
    UpdateConfig, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings,
    package_configs::ProjectConfig,
    registries::{RegistryDeclaration, RegistryEntry},
};
use crate::{
    AuditLevel, CatalogMode, ColorMode, Config, GlobalShims, GlobalShimsSetting, HoistingLimits,
    LinkWorkspacePackages, NodeLinker, NodePackageMapType, PmOnFail, ResolutionMode, RuntimeOnFail,
    ScriptsPrependNodePath, ShimPolicy, TrustPolicy,
    api::{EnvVar, GetHomeDir},
};
use indexmap::IndexMap;
use pipe_trait::Pipe;
use pnpm_lockfile::{RegistryOptions, RegistryServerType};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_store_dir::StoreDir;
use pnpm_workspace_state::{ConfigDependency, ConfigDependencyDetail};
use pretty_assertions::assert_eq;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

thread_local! {
    static CASE: std::cell::RefCell<Option<(Vec<&'static str>, &'static str, &'static str)>> =
        const { std::cell::RefCell::new(None) };
}

/// The settings [`WorkspaceSettings::from_resolved`] deliberately leaves
/// unset, each because pnpm reports the same thing under another name. See
/// that method's documentation for why each one is here.
const UNREPORTED_SETTINGS: &[&str] = &[
    // Shapes only a file has; the resolved form reports elsewhere.
    "registries",
    "namedRegistries",
    "catalog",
    "onlyBuiltDependencies",
    "neverBuiltDependencies",
    "ignoredBuiltDependencies",
    // Deprecated spellings of a canonical key.
    "maxsockets",
    "noproxy",
    // npm's spelling, which resolves into `httpsProxy` / `httpProxy`.
    "proxy",
    "updateConfig",
    "auditLevel",
    "auditConfig",
    "cleanupUnusedCatalogs",
    "virtualStoreType",
    // Never read from a project file.
    "_auth",
];

mod configuration_parses_common_settings_from;

mod configuration_from_resolved_reports_every;

mod behavior_color_accepts_boolean_compatibility;

mod behavior_trust_policy_yaml_values;

mod runtime;

mod files;

mod workspace_settings;

mod reporting;

mod integrity;

mod dependencies;

mod security;

mod manifests;

mod lockfile;

mod authorization;
