pub mod package_configs;
pub mod registries;
pub use error::LoadWorkspaceYamlError;
pub use sections::{
    AllowBuild, AuditSettings, CargoSettings, PackageExtension, PeerDependencyMeta,
    PeerDependencyRules, PnpmfileSetting, PythonSettings, RemoteSideEffectsCacheSettings,
    SideEffectsCacheSetting, SideEffectsCacheSettings, TaskSettings, UpdateConfig, UpdateSettings,
    decided_allow_builds,
};
pub use settings::WorkspaceSettings;

use crate::{
    AuditConfig, AuditLevel, CatalogMode, Config, HoistingLimits, InitType, LinkWorkspacePackages,
    NodeLinker, NodePackageMapType, PackageImportMethod, PmOnFail, ResolutionMode, RuntimeOnFail,
    SaveWorkspaceProtocol, ScriptsPrependNodePath, TrustPolicy, VerifyDepsBeforeRun,
    VirtualStoreType,
    api::{EnvVar, GetCurrentDir, GetHomeDir, LinkProbe},
    config_types::is_config_file_key,
    known_settings::{SCHEMA_DIRECTIVE_KEY, annotate_unknown_setting, is_known_setting_key},
    naming_cases::{is_camel_case, to_camel_case, to_kebab_case},
    proxy_keys::{ProxyKeys, ProxyValue},
    refused_keys::{is_refused_by_a_project_manifest, where_refused_key_belongs},
    resolve_child_concurrency,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use package_configs::PackageConfigsSetting;
use pipe_trait::Pipe;
use pnpm_env_replace::env_replace_lossy;
use pnpm_network::redact_and_sanitize;
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_store_dir::StoreDir;
use pnpm_workspace_state::ConfigDependency;
use registries::RegistryEntry;
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// The keys of a project's `pnpm-workspace.yaml` that set nothing, bucketed
/// by why: refused values a project may not contribute, keys naming no
/// setting any supported pnpm reads, and kebab-case spellings of keys pnpm
/// only reads in camelCase.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WorkspaceKeyIssues {
    pub refused: Vec<String>,
    pub unrecognized: Vec<String>,
    pub non_camel_case: Vec<String>,
}

impl WorkspaceKeyIssues {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.refused.is_empty() && self.unrecognized.is_empty() && self.non_camel_case.is_empty()
    }
}

/// Basename of the file pnpm reads; exported for test use.
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Basename of pnpm's global config file inside `<configDir>`.
pub const GLOBAL_CONFIG_YAML_FILENAME: &str = "config.yaml";

/// Overwrite `target` when the layer set the field, leaving it untouched
/// otherwise.
fn overlay<Setting>(target: &mut Setting, value: Option<Setting>) {
    if let Some(value) = value {
        *target = value;
    }
}

/// [`overlay`] for a target that is itself optional: an unset field leaves
/// whatever the previous layer recorded, including its absence.
fn overlay_some<Setting>(target: &mut Option<Setting>, value: Option<Setting>) {
    if value.is_some() {
        *target = value;
    }
}

/// The dropped keys of a global `config.yaml`, in the four buckets its
/// warnings report.
#[derive(Default)]
struct DroppedKeys {
    movable: Vec<String>,
    unrecognized: Vec<String>,
    nowhere: Vec<String>,
    kebab_case: Vec<String>,
}

impl DroppedKeys {
    fn warn(self, path: &Path) {
        let DroppedKeys { movable, unrecognized, nowhere, kebab_case } = self;

        let path = path.display();
        if !movable.is_empty() {
            let movable = movable.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings cannot be set in the global config file ("{path}") and were ignored: {movable}. Move them to a project-level pnpm-workspace.yaml. To share these settings across projects, use config dependencies: https://pnpm.io/11.x/config-dependencies"#,
            );
        }
        if !unrecognized.is_empty() {
            let unrecognized = unrecognized.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings in the global config file ("{path}") are not recognized by this version of pnpm and were ignored: {unrecognized}."#,
            );
        }
        if !nowhere.is_empty() {
            let nowhere = nowhere.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings cannot be set in the global config file ("{path}") and were ignored: {nowhere}."#,
            );
        }
        if !kebab_case.is_empty() {
            let kebab_case = kebab_case.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"The following settings in the global config file ("{path}") were ignored because they are not written in camelCase: {kebab_case}."#,
            );
        }
    }

    /// A key this file cannot carry belongs in a project file, is spelled in
    /// kebab-case, belongs nowhere, or is not a setting at all.
    fn classify(&mut self, key: &str) {
        if is_config_file_key(&to_kebab_case(key)) {
            if !is_camel_case(key) {
                self.kebab_case.push(format!(r#""{key}" (use "{}")"#, to_camel_case(key)));
            }
            return;
        }
        if is_refused_by_a_project_manifest(key) {
            let belongs = where_refused_key_belongs(&to_camel_case(key));
            self.nowhere.push(format!(r#""{key}" ({belongs})"#));
        } else if is_known_setting_key(key) {
            self.movable.push(format!(r#""{key}""#));
        } else {
            self.unrecognized.push(annotate_unknown_setting(key));
        }
    }
}

/// The settings [`WorkspaceSettings`] and [`Config`] name identically and
/// hold at the same type, so a value moves between them by plain assignment.
///
/// Expands to a call of `$mac!` with the field list.
/// [`WorkspaceSettings::apply_to`] and [`WorkspaceSettings::from_resolved`]
/// are inverses over it, and naming a setting here is what keeps them so:
/// teaching one direction about a setting teaches the other.
macro_rules! identically_named_settings {
    ($mac:ident) => {
        $mac! {
            bail, ci, update_notifier, color, embed_readme, ignore_workspace_root_check,
            optional, package_lock, pending, recursive_install, reverse,
            stream, aggregate_output, use_stderr, ignore_workspace, shell_emulator,
            skip_manifest_obfuscation, sort, use_beta_cli,
            hoist, shamefully_hoist,
            node_linker, node_experimental_package_map, node_package_map_type,
            symlink, package_import_method, modules_cache_max_age,
            virtual_store_dir_max_length,
            peers_suffix_max_length,
            lockfile, prefer_frozen_lockfile,
            deploy_all_files, force_legacy_deploy, shared_workspace_lockfile,
            merge_git_branch_lockfiles, merge_git_branch_lockfiles_branch_pattern,
            offline, prefer_offline,
            lockfile_include_tarball_url,
            auto_install_peers, auto_install_peers_from_highest_match,
            exclude_links_from_lockfile,
            optimistic_repeat_install,
            init_package_manager,
            init_type,
            hoist_workspace_packages,
            extend_node_path,
            hoisting_limits, external_dependencies,
            dedupe_peer_dependents, dedupe_peers,
            dedupe_direct_deps, dedupe_injected_deps,
            strict_peer_dependencies, ignore_compatibility_db,
            resolve_peers_from_workspace_root, verify_store_integrity,
            strict_store_pkg_content_check, frozen_store,
            include_workspace_root,
            ignore_workspace_cycles, disallow_workspace_cycles,
            verify_deps_before_run,
            block_exotic_subdeps,
            link_workspace_packages,
            save_workspace_protocol,
            inject_workspace_packages,
            prefer_workspace_packages,
            side_effects_cache_readonly,
            fetch_retries, fetch_retry_factor,
            fetch_retry_mintimeout, fetch_retry_maxtimeout,
            network_concurrency, fetch_timeout,
            fetch_warn_timeout_ms, fetch_min_speed_ki_bps, user_agent,
            enable_global_virtual_store,
            virtual_store_only, enable_modules_dir,
            git_shallow_hosts,
            test_pattern, changed_files_ignore_pattern, legacy_dir_filtering,
            sync_injected_deps_after_scripts,
            resolution_mode, catalog_mode, catalog_prune,
            minimum_release_age_exclude_prune, save_peer, save_exact,
            registry_supports_time_field,
            allowed_deprecated_versions, update_config, peer_dependency_rules,
            enable_pre_post_scripts, dlx_cache_max_age,
            allow_unused_patches, tasks, pipelines,
        }
    };
}

fn global_shims_setting(config: &Config) -> crate::GlobalShimsSetting {
    crate::GlobalShimsSetting::Entries(
        config
            .global_shims
            .entries()
            .map(|(name, policy)| {
                // `Off` has no named spelling; it is written as the
                // `false` shorthand.
                let value = match policy {
                    crate::ShimPolicy::Off => crate::ShimPolicyValue::Toggle(false),
                    crate::ShimPolicy::Auto => {
                        crate::ShimPolicyValue::Named(crate::NamedShimPolicy::Auto)
                    }
                    crate::ShimPolicy::Prompt => {
                        crate::ShimPolicyValue::Named(crate::NamedShimPolicy::Prompt)
                    }
                    crate::ShimPolicy::Always => {
                        crate::ShimPolicyValue::Named(crate::NamedShimPolicy::Always)
                    }
                };
                (name.to_string(), value)
            })
            .collect(),
    )
}

/// Reads and writes as resolved, in the boolean shorthand when they agree
/// and no remote cache is configured. That is the shape pnpm reports and
/// the one a hook is likeliest to assign.
fn side_effects_cache_setting(config: &Config) -> SideEffectsCacheSetting {
    let read = config.side_effects_cache_read();
    let write = config.side_effects_cache_write();
    if read == write && config.remote_side_effects_cache.is_none() {
        SideEffectsCacheSetting::Enabled(read)
    } else {
        SideEffectsCacheSetting::Settings(Box::new(SideEffectsCacheSettings {
            read: Some(read),
            write: Some(write),
            remote: config.remote_side_effects_cache.clone(),
        }))
    }
}

/// The pattern a source still sets under `key`, or `default` when none
/// does.
fn explicit_or_default(
    config: &Config,
    key: &str,
    default: Option<&[String]>,
) -> Option<Vec<String>> {
    explicit_pattern(config, key).or_else(|| default.map(<[String]>::to_vec))
}

/// The pattern a source still sets under `key`.
fn explicit_pattern(config: &Config, key: &str) -> Option<Vec<String>> {
    config.explicit_settings.get(key).and_then(|value| serde_json::from_value(value.clone()).ok())
}

/// Warn that a file sets both the `audit` section and the deprecated
/// setting `deprecated` it supersedes.
fn warn_deprecated_pairing(also_set: bool, deprecated: &str) {
    if !also_set {
        return;
    }
    tracing::warn!(
        target: "pacquet::config",
        r#"Both the "audit" and "{deprecated}" settings are set. The deprecated "{deprecated}" setting is ignored in favor of "audit"."#,
    );
}

/// Join `fragment` onto `base` the way pnpm's `path.join` does: concatenate
/// with the separator, then normalize. Node treats every argument after the
/// first as a fragment, so [`Path::join`] is the wrong primitive here — it
/// lets a fragment that parses as rooted (`//bin`) or drive-prefixed
/// (`C:bin`, drive-relative on Windows) replace `base` outright.
fn join_fragment(base: &Path, fragment: &str) -> PathBuf {
    let mut joined = base.as_os_str().to_os_string();
    joined.push(std::path::MAIN_SEPARATOR_STR);
    joined.push(fragment);
    pnpm_fs::lexical_normalize(Path::new(&joined))
}

fn resolve(base: &Path, value: &str) -> PathBuf {
    let candidate = Path::new(value);
    if candidate.is_absolute() { candidate.to_path_buf() } else { base.join(candidate) }
}

pub(crate) fn find_workspace_manifest(start: &Path) -> Option<PathBuf> {
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

fn path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
fn opt_path(value: Option<&Path>) -> Option<String> {
    value.map(path)
}
/// The value a source set for `key`, as written, or `None` when
/// nothing set it.
fn as_set<Setting: serde::de::DeserializeOwned>(config: &Config, key: &str) -> Option<Setting> {
    config.explicit_settings.get(key).cloned().and_then(|value| serde_json::from_value(value).ok())
}

#[cfg(test)]
mod tests;

mod sections;

mod validation;

mod workspace_scope;

mod env;

mod environment_values;
use environment_values::{
    has_env_placeholder, no_proxy_scalar, normalize_registry_url, substitute_json_string,
    substitute_optional_inner_string, substitute_optional_string, substitute_optional_string_map,
    substitute_registry_entries,
};

mod apply;

mod resolved;

mod reset;

mod settings;

mod error;

mod credentials;
use credentials::{redact_registry_url, registry_url_has_userinfo};
