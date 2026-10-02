//! Read `PNPM_CONFIG_*` / `pnpm_config_*` environment variables into a
//! [`WorkspaceSettings`] overlay.
//!
//! Reads `pnpm_config_<key>` (or its `PNPM_CONFIG_<KEY>` uppercase form)
//! for every key in the schema and applies it to the config *after*
//! `pnpm-workspace.yaml`.
//! The command line's `--config.<key>=<value>` values read through the same
//! field list; see [`crate::Config::cli_setting_values`].
//!
//! Pacquet does NOT read `npm_config_*` / `NPM_CONFIG_*` env vars (with
//! the exception of `NPM_CONFIG_WORKSPACE_DIR`, a fallback of the
//! workspace-dir override that [`crate::Config::current`] handles on its
//! own, see [`pnpm_workspace::WORKSPACE_DIR_ENV_VARS`]). pnpm stopped honouring those
//! too; the only remaining `npm_config_*` lookup in pnpm is `userconfig`
//! as a low-priority auth-file fallback.

use crate::{
    AuditLevel, CatalogMode, ColorMode, HoistingLimits, InitType, LogLevel, NodeLinkerSetting,
    NodePackageMapType, PackageImportMethod, PmOnFail, ReporterType, ResolutionMode, RuntimeOnFail,
    SaveWorkspaceProtocol, ScriptsPrependNodePath, TrustPolicy, VerifyDepsBeforeRun,
    VirtualStoreType, WorkspaceSettings, api::EnvVar,
};
use parse::{parse_json, parse_json_or_string, parse_tri_array};
use std::collections::BTreeMap;
use string_reader::{StringReader, dotted_value, read_env, read_env_allow_empty};

mod parse;
mod string_reader;

macro_rules! json_field {
    ($settings:ident, $reader:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $reader.value($suffix)
            && let Some(v) = parse_json(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! string_field {
    ($settings:ident, $reader:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $reader.value($suffix) {
            $settings.$field = Some(s);
        }
    };
}
// Like `string_field!`, but keeps an empty value as `Some("")` instead
// of treating it as unset. For settings where `""` is observably
// different from unset. See [`read_env_allow_empty`].
macro_rules! string_field_allow_empty {
    ($settings:ident, $reader:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $reader.value_allow_empty($suffix) {
            $settings.$field = Some(s);
        }
    };
}
macro_rules! enum_field {
    ($settings:ident, $reader:expr, $field:ident, $suffix:literal, $ty:ty) => {
        if let Some(s) = $reader.value($suffix)
            && let Some(v) = parse_json_or_string::<$ty>(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! tri_array_field {
    ($settings:ident, $reader:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $reader.value($suffix)
            && let Some(v) = parse_tri_array(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
// Env vars cannot express the "explicit null clears" state that
// yaml supports (an empty value reads as unset — see `read_env`),
// so a present env var always lands as `Some(Some(s))`, never
// `Some(None)`. Same limitation as `tri_array_field!`.
macro_rules! tri_string_field {
    ($settings:ident, $reader:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $reader.value($suffix) {
            $settings.$field = Some(Some(s));
        }
    };
}

impl WorkspaceSettings {
    /// Build a [`WorkspaceSettings`] from `PNPM_CONFIG_*` env vars.
    ///
    /// Env vars are read for the full schema, not just config-file
    /// keys — `PNPM_CONFIG_HOIST=false`, `PNPM_CONFIG_NODE_LINKER=hoisted`
    /// etc. all work (no [`Self::clear_workspace_only_fields`] call).
    /// Apply the returned settings via [`Self::apply_to`] *after*
    /// `pnpm-workspace.yaml` so env vars win over yaml.
    #[must_use]
    pub fn from_pnpm_config_env<Sys: EnvVar>() -> Self {
        Self::from_reader(&StringReader {
            read: &|suffix| read_env::<Sys>(suffix),
            read_allow_empty: &|suffix| read_env_allow_empty::<Sys>(suffix),
        })
    }

    /// Build the overlay from a flat map of string settings, keyed by the
    /// setting name in either kebab-case or camelCase.
    ///
    /// The command line's `--config.<setting>=<value>` tokens are the same
    /// shape of input as the environment, one string per setting, so they
    /// read through the field list below rather than a second copy of it.
    #[must_use]
    pub(crate) fn from_string_values(values: &BTreeMap<String, String>) -> Self {
        Self::from_reader(&StringReader {
            read: &|suffix| dotted_value(values, suffix).filter(|value| !value.is_empty()),
            read_allow_empty: &|suffix| dotted_value(values, suffix),
        })
    }

    fn from_reader(reader: &StringReader<'_>) -> Self {
        let mut settings = WorkspaceSettings::default();

        settings.read_general_env(reader);
        settings.read_layout_env(reader);
        settings.read_lockfile_env(reader);
        settings.read_registry_env(reader);
        settings.read_resolution_env(reader);
        settings.read_network_env(reader);
        settings.read_scripts_env(reader);
        settings.read_workspace_env(reader);
        settings.read_trust_env(reader);
        settings.read_init_env(reader);
        settings.read_verification_env(reader);
        settings.read_save_env(reader);

        settings
    }
    fn read_general_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, bail, "BAIL");
        json_field!(settings, reader, ci, "CI");
        json_field!(settings, reader, progress, "PROGRESS");
        json_field!(settings, reader, update_notifier, "UPDATE_NOTIFIER");
        enum_field!(settings, reader, color, "COLOR", ColorMode);
        enum_field!(settings, reader, loglevel, "LOGLEVEL", LogLevel);
        enum_field!(settings, reader, reporter, "REPORTER", ReporterType);
        json_field!(settings, reader, embed_readme, "EMBED_README");
        json_field!(settings, reader, ignore_pnpmfile, "IGNORE_PNPMFILE");
        json_field!(settings, reader, ignore_workspace_root_check, "IGNORE_WORKSPACE_ROOT_CHECK");
        json_field!(settings, reader, optional, "OPTIONAL");
        json_field!(settings, reader, package_lock, "PACKAGE_LOCK");
        json_field!(settings, reader, pending, "PENDING");
        json_field!(settings, reader, recursive_install, "RECURSIVE_INSTALL");
        json_field!(settings, reader, reverse, "REVERSE");
        json_field!(settings, reader, stream, "STREAM");
        json_field!(settings, reader, aggregate_output, "AGGREGATE_OUTPUT");
        json_field!(settings, reader, reporter_hide_prefix, "REPORTER_HIDE_PREFIX");
        json_field!(settings, reader, use_stderr, "USE_STDERR");
        json_field!(settings, reader, ignore_workspace, "IGNORE_WORKSPACE");
        json_field!(settings, reader, shell_emulator, "SHELL_EMULATOR");
        json_field!(settings, reader, skip_manifest_obfuscation, "SKIP_MANIFEST_OBFUSCATION");
        json_field!(settings, reader, sort, "SORT");
        json_field!(settings, reader, use_beta_cli, "USE_BETA_CLI");
    }

    fn read_macos_backup_env(&mut self, reader: &StringReader<'_>) {
        if let Some(value) =
            reader.value("MACOS_BACKUP_EXCLUDE_MODULES_DIR").and_then(|value| parse_json(&value))
        {
            self.macos_backup.get_or_insert_default().exclude_modules_dir = Some(value);
        }
        if let Some(value) =
            reader.value("MACOS_BACKUP_EXCLUDE_STORE_DIR").and_then(|value| parse_json(&value))
        {
            self.macos_backup.get_or_insert_default().exclude_store_dir = Some(value);
        }
    }

    fn read_layout_env(&mut self, reader: &StringReader<'_>) {
        self.read_macos_backup_env(reader);
        let settings = self;
        json_field!(settings, reader, hoist, "HOIST");
        tri_array_field!(settings, reader, hoist_pattern, "HOIST_PATTERN");
        tri_array_field!(settings, reader, public_hoist_pattern, "PUBLIC_HOIST_PATTERN");
        json_field!(settings, reader, shamefully_hoist, "SHAMEFULLY_HOIST");
        string_field!(settings, reader, store_dir, "STORE_DIR");
        string_field!(settings, reader, state_dir, "STATE_DIR");
        string_field!(settings, reader, modules_dir, "MODULES_DIR");
        enum_field!(settings, reader, node_linker, "NODE_LINKER", NodeLinkerSetting);
        json_field!(
            settings,
            reader,
            node_experimental_package_map,
            "NODE_EXPERIMENTAL_PACKAGE_MAP"
        );
        enum_field!(
            settings,
            reader,
            node_package_map_type,
            "NODE_PACKAGE_MAP_TYPE",
            NodePackageMapType
        );
        json_field!(settings, reader, symlink, "SYMLINK");
        settings.read_virtual_store_env(reader);
    }

    fn read_virtual_store_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        string_field!(settings, reader, virtual_store_dir, "VIRTUAL_STORE_DIR");
        enum_field!(settings, reader, virtual_store_type, "VIRTUAL_STORE_TYPE", VirtualStoreType);
        json_field!(settings, reader, enable_global_virtual_store, "ENABLE_GLOBAL_VIRTUAL_STORE");
        json_field!(settings, reader, virtual_store_only, "VIRTUAL_STORE_ONLY");
        json_field!(settings, reader, enable_modules_dir, "ENABLE_MODULES_DIR");
        json_field!(settings, reader, global_shims, "GLOBAL_SHIMS");
        string_field!(settings, reader, global_virtual_store_dir, "GLOBAL_VIRTUAL_STORE_DIR");
        string_field!(settings, reader, global_dir, "GLOBAL_DIR");
        string_field!(settings, reader, global_bin_dir, "GLOBAL_BIN_DIR");
        enum_field!(
            settings,
            reader,
            package_import_method,
            "PACKAGE_IMPORT_METHOD",
            PackageImportMethod
        );
        json_field!(settings, reader, modules_cache_max_age, "MODULES_CACHE_MAX_AGE");
        json_field!(settings, reader, virtual_store_dir_max_length, "VIRTUAL_STORE_DIR_MAX_LENGTH");
        json_field!(settings, reader, peers_suffix_max_length, "PEERS_SUFFIX_MAX_LENGTH");
    }

    fn read_lockfile_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, lockfile, "LOCKFILE");
        string_field!(settings, reader, lockfile_dir, "LOCKFILE_DIR");
        json_field!(settings, reader, prefer_frozen_lockfile, "PREFER_FROZEN_LOCKFILE");
        json_field!(settings, reader, prefer_symlinked_executables, "PREFER_SYMLINKED_EXECUTABLES");
        json_field!(settings, reader, frozen_lockfile, "FROZEN_LOCKFILE");
        json_field!(settings, reader, deploy_all_files, "DEPLOY_ALL_FILES");
        json_field!(settings, reader, force_legacy_deploy, "FORCE_LEGACY_DEPLOY");
        json_field!(settings, reader, shared_workspace_lockfile, "SHARED_WORKSPACE_LOCKFILE");
        json_field!(settings, reader, git_branch_lockfile, "GIT_BRANCH_LOCKFILE");
        json_field!(settings, reader, merge_git_branch_lockfiles, "MERGE_GIT_BRANCH_LOCKFILES");
        json_field!(
            settings,
            reader,
            merge_git_branch_lockfiles_branch_pattern,
            "MERGE_GIT_BRANCH_LOCKFILES_BRANCH_PATTERN"
        );
    }

    fn read_registry_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, offline, "OFFLINE");
        json_field!(settings, reader, prefer_offline, "PREFER_OFFLINE");
        json_field!(settings, reader, lockfile_include_tarball_url, "LOCKFILE_INCLUDE_TARBALL_URL");
        string_field!(settings, reader, registry, "REGISTRY");
        // Unlike its `string_field!` neighbors, `scope` keeps an empty value;
        // see [`read_env_allow_empty`] for why.
        if let Some(scope) = reader.value_allow_empty("SCOPE") {
            settings.scope = Some(scope);
        }
        string_field!(settings, reader, pnpr_server, "PNPR_SERVER");
        string_field!(settings, reader, https_proxy, "HTTPS_PROXY");
        string_field!(settings, reader, http_proxy, "HTTP_PROXY");
        string_field!(settings, reader, proxy, "PROXY");
        if let Some(value) = reader.value("NO_PROXY") {
            settings.no_proxy = Some(serde_json::Value::String(value));
        }
        if let Some(value) = reader.value("NOPROXY") {
            settings.noproxy = Some(serde_json::Value::String(value));
        }
    }

    fn read_resolution_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, auto_install_peers, "AUTO_INSTALL_PEERS");
        json_field!(
            settings,
            reader,
            auto_install_peers_from_highest_match,
            "AUTO_INSTALL_PEERS_FROM_HIGHEST_MATCH"
        );
        json_field!(settings, reader, exclude_links_from_lockfile, "EXCLUDE_LINKS_FROM_LOCKFILE");
        json_field!(settings, reader, hoist_workspace_packages, "HOIST_WORKSPACE_PACKAGES");
        enum_field!(settings, reader, hoisting_limits, "HOISTING_LIMITS", HoistingLimits);
        json_field!(settings, reader, external_dependencies, "EXTERNAL_DEPENDENCIES");
        json_field!(settings, reader, dedupe_peer_dependents, "DEDUPE_PEER_DEPENDENTS");
        json_field!(settings, reader, dedupe_peers, "DEDUPE_PEERS");
        json_field!(settings, reader, auto_dedupe, "AUTO_DEDUPE");
        json_field!(settings, reader, dedupe_direct_deps, "DEDUPE_DIRECT_DEPS");
        json_field!(settings, reader, prefer_workspace_packages, "PREFER_WORKSPACE_PACKAGES");
        json_field!(settings, reader, dedupe_injected_deps, "DEDUPE_INJECTED_DEPS");
        json_field!(settings, reader, strict_peer_dependencies, "STRICT_PEER_DEPENDENCIES");
        json_field!(settings, reader, ignore_compatibility_db, "IGNORE_COMPATIBILITY_DB");
        json_field!(
            settings,
            reader,
            resolve_peers_from_workspace_root,
            "RESOLVE_PEERS_FROM_WORKSPACE_ROOT"
        );
        json_field!(settings, reader, block_exotic_subdeps, "BLOCK_EXOTIC_SUBDEPS");
        json_field!(settings, reader, verify_store_integrity, "VERIFY_STORE_INTEGRITY");
        json_field!(
            settings,
            reader,
            strict_store_pkg_content_check,
            "STRICT_STORE_PKG_CONTENT_CHECK"
        );
        json_field!(settings, reader, include_workspace_root, "INCLUDE_WORKSPACE_ROOT");
        json_field!(settings, reader, fail_if_no_match, "FAIL_IF_NO_MATCH");
        json_field!(settings, reader, ignore_workspace_cycles, "IGNORE_WORKSPACE_CYCLES");
        json_field!(settings, reader, disallow_workspace_cycles, "DISALLOW_WORKSPACE_CYCLES");
        json_field!(settings, reader, side_effects_cache, "SIDE_EFFECTS_CACHE");
        json_field!(settings, reader, side_effects_cache_readonly, "SIDE_EFFECTS_CACHE_READONLY");
    }

    fn read_network_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, fetch_retries, "FETCH_RETRIES");
        json_field!(settings, reader, fetch_retry_factor, "FETCH_RETRY_FACTOR");
        json_field!(settings, reader, fetch_retry_mintimeout, "FETCH_RETRY_MINTIMEOUT");
        json_field!(settings, reader, fetch_retry_maxtimeout, "FETCH_RETRY_MAXTIMEOUT");
        json_field!(settings, reader, network_concurrency, "NETWORK_CONCURRENCY");
        json_field!(settings, reader, maxsockets, "MAXSOCKETS");
        json_field!(settings, reader, max_sockets, "MAX_SOCKETS");
        json_field!(settings, reader, fetch_timeout, "FETCH_TIMEOUT");
        json_field!(settings, reader, fetch_warn_timeout_ms, "FETCH_WARN_TIMEOUT_MS");
        json_field!(settings, reader, fetch_min_speed_ki_bps, "FETCH_MIN_SPEED_KI_BPS");
        string_field!(settings, reader, user_agent, "USER_AGENT");
    }

    fn read_scripts_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, patched_dependencies, "PATCHED_DEPENDENCIES");
        json_field!(settings, reader, allow_unused_patches, "ALLOW_UNUSED_PATCHES");
        string_field!(settings, reader, patches_dir, "PATCHES_DIR");
        string_field!(settings, reader, global_pnpmfile, "GLOBAL_PNPMFILE");
        enum_field!(settings, reader, pnpmfile, "PNPMFILE", crate::PnpmfileSetting);
        json_field!(settings, reader, allow_builds, "ALLOW_BUILDS");
        json_field!(settings, reader, dangerously_allow_all_builds, "DANGEROUSLY_ALLOW_ALL_BUILDS");
        json_field!(settings, reader, strict_dep_builds, "STRICT_DEP_BUILDS");
        json_field!(settings, reader, ignore_scripts, "IGNORE_SCRIPTS");
        json_field!(settings, reader, git_checks, "GIT_CHECKS");
        json_field!(settings, reader, publish_wait_timeout, "PUBLISH_WAIT_TIMEOUT");
        string_field_allow_empty!(settings, reader, tag_version_prefix, "TAG_VERSION_PREFIX");
        json_field!(settings, reader, engine_strict, "ENGINE_STRICT");
        json_field!(settings, reader, force_ignores_platform, "FORCE_IGNORES_PLATFORM");
        string_field!(settings, reader, node_version, "NODE_VERSION");
        enum_field!(settings, reader, runtime_on_fail, "RUNTIME_ON_FAIL", RuntimeOnFail);
        json_field!(settings, reader, node_download_mirrors, "NODE_DOWNLOAD_MIRRORS");
        enum_field!(
            settings,
            reader,
            scripts_prepend_node_path,
            "SCRIPTS_PREPEND_NODE_PATH",
            ScriptsPrependNodePath
        );
        json_field!(settings, reader, enable_pre_post_scripts, "ENABLE_PRE_POST_SCRIPTS");
        tri_string_field!(settings, reader, script_shell, "SCRIPT_SHELL");
        settings.node_options = reader.value_allow_empty("NODE_OPTIONS").map(Some);
        json_field!(settings, reader, unsafe_perm, "UNSAFE_PERM");
        json_field!(settings, reader, child_concurrency, "CHILD_CONCURRENCY");
        json_field!(settings, reader, workspace_concurrency, "WORKSPACE_CONCURRENCY");
        json_field!(settings, reader, concurrency_groups, "CONCURRENCY_GROUPS");
        json_field!(settings, reader, git_shallow_hosts, "GIT_SHALLOW_HOSTS");
    }

    fn read_workspace_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, test_pattern, "TEST_PATTERN");
        json_field!(settings, reader, legacy_dir_filtering, "LEGACY_DIR_FILTERING");
        json_field!(
            settings,
            reader,
            sync_injected_deps_after_scripts,
            "SYNC_INJECTED_DEPS_AFTER_SCRIPTS"
        );
        json_field!(settings, reader, changed_files_ignore_pattern, "CHANGED_FILES_IGNORE_PATTERN");
        json_field!(settings, reader, supported_architectures, "SUPPORTED_ARCHITECTURES");
        json_field!(settings, reader, tools, "TOOLS");
        json_field!(
            settings,
            reader,
            ignored_optional_dependencies,
            "IGNORED_OPTIONAL_DEPENDENCIES"
        );
        json_field!(settings, reader, overrides, "OVERRIDES");
        json_field!(settings, reader, package_extensions, "PACKAGE_EXTENSIONS");
    }

    fn read_trust_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        string_field!(settings, reader, cache_dir, "CACHE_DIR");
        json_field!(settings, reader, dlx_cache_max_age, "DLX_CACHE_MAX_AGE");
        json_field!(settings, reader, minimum_release_age, "MINIMUM_RELEASE_AGE");
        json_field!(settings, reader, minimum_release_age_exclude, "MINIMUM_RELEASE_AGE_EXCLUDE");
        json_field!(
            settings,
            reader,
            minimum_release_age_ignore_missing_time,
            "MINIMUM_RELEASE_AGE_IGNORE_MISSING_TIME"
        );
        json_field!(settings, reader, minimum_release_age_strict, "MINIMUM_RELEASE_AGE_STRICT");
        json_field!(settings, reader, trust_lockfile, "TRUST_LOCKFILE");
        enum_field!(settings, reader, trust_policy, "TRUST_POLICY", TrustPolicy);
        enum_field!(settings, reader, pm_on_fail, "PM_ON_FAIL", PmOnFail);
    }

    fn read_init_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, reader, init_package_manager, "INIT_PACKAGE_MANAGER");
        enum_field!(settings, reader, init_type, "INIT_TYPE", InitType);
        string_field!(settings, reader, init_author_name, "INIT_AUTHOR_NAME");
        string_field!(settings, reader, init_author_email, "INIT_AUTHOR_EMAIL");
        string_field!(settings, reader, init_author_url, "INIT_AUTHOR_URL");
        string_field!(settings, reader, init_license, "INIT_LICENSE");
        string_field!(settings, reader, init_version, "INIT_VERSION");
    }

    fn read_verification_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        // pnpm applies this env var on presence alone (`!= null`) and
        // assigns the raw value without validation, so presence always
        // overrides the other config layers: an empty value assigns an
        // empty string — falsy there, the gate is off — and an
        // unrecognized value is truthy — the check runs but matches no
        // action. `value` filters empty values, so read the raw one.
        if let Some(s) = reader.value_allow_empty("VERIFY_DEPS_BEFORE_RUN") {
            settings.verify_deps_before_run = Some(if s.is_empty() {
                VerifyDepsBeforeRun::False
            } else {
                parse_json_or_string::<VerifyDepsBeforeRun>(&s).unwrap_or(VerifyDepsBeforeRun::True)
            });
        }
    }

    fn read_save_env(&mut self, reader: &StringReader<'_>) {
        let settings = self;
        enum_field!(settings, reader, audit_level, "AUDIT_LEVEL", AuditLevel);
        json_field!(settings, reader, audit_config, "AUDIT_CONFIG");
        json_field!(settings, reader, trust_policy_exclude, "TRUST_POLICY_EXCLUDE");
        json_field!(settings, reader, trust_policy_ignore_after, "TRUST_POLICY_IGNORE_AFTER");
        enum_field!(settings, reader, resolution_mode, "RESOLUTION_MODE", ResolutionMode);
        enum_field!(settings, reader, catalog_mode, "CATALOG_MODE", CatalogMode);
        string_field!(settings, reader, save_catalog_name, "SAVE_CATALOG_NAME");
        if let Some(save_prefix) = reader.value_allow_empty("SAVE_PREFIX") {
            settings.save_prefix = Some(save_prefix);
        }
        json_field!(settings, reader, save_exact, "SAVE_EXACT");
        json_field!(settings, reader, save_peer, "SAVE_PEER");
        json_field!(settings, reader, save_types, "SAVE_TYPES");
        enum_field!(
            settings,
            reader,
            save_workspace_protocol,
            "SAVE_WORKSPACE_PROTOCOL",
            SaveWorkspaceProtocol
        );
        json_field!(settings, reader, registry_supports_time_field, "REGISTRY_SUPPORTS_TIME_FIELD");
        json_field!(settings, reader, allowed_deprecated_versions, "ALLOWED_DEPRECATED_VERSIONS");
        json_field!(settings, reader, update_config, "UPDATE_CONFIG");
        json_field!(settings, reader, peer_dependency_rules, "PEER_DEPENDENCY_RULES");
    }
}

#[cfg(test)]
mod tests;
