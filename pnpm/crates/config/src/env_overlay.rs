//! Read `PNPM_CONFIG_*` / `pnpm_config_*` environment variables into a
//! [`WorkspaceSettings`] overlay.
//!
//! Reads `pnpm_config_<key>` (or its `PNPM_CONFIG_<KEY>` uppercase form)
//! for every key in the schema and applies it to the config *after*
//! `pnpm-workspace.yaml`. That ordering means env vars override yaml.
//!
//! Pacquet does NOT read `npm_config_*` / `NPM_CONFIG_*` env vars (with
//! the exception of `NPM_CONFIG_WORKSPACE_DIR`, which has its own narrow
//! handler in [`crate::Config::current`]). pnpm stopped honouring those
//! too; the only remaining `npm_config_*` lookup in pnpm is `userconfig`
//! as a low-priority auth-file fallback.

use crate::{
    AuditLevel, CatalogMode, ColorMode, HoistingLimits, InitType, NodeLinker, NodePackageMapType,
    PackageImportMethod, PmOnFail, ResolutionMode, RuntimeOnFail, SaveWorkspaceProtocol,
    ScriptsPrependNodePath, TrustPolicy, VerifyDepsBeforeRun, VirtualStoreType, WorkspaceSettings,
    api::EnvVar,
};
use serde::de::DeserializeOwned;

/// Read an env var by suffix, accepting both `PNPM_CONFIG_<UPPER>` and
/// `pnpm_config_<lower>`. Empty values are treated as unset.
fn read_env<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper).or_else(|| Sys::var(&lower)).filter(|value| !value.is_empty())
}

/// Read an env var by suffix, keeping an empty value as `Some("")`.
///
/// pnpm's own env pass only skips a variable that is absent, never one
/// that is empty, so an empty value clobbers lower-priority layers. For
/// nearly every setting an empty value is indistinguishable from an unset
/// one, which is why [`read_env`] drops it. Two settings are exceptions,
/// where `""` is observably different from unset:
///
/// - `savePrefix`: `""` is the value that selects an exact version pin.
/// - `scope`: `""` must override a scope from the global `config.yaml` so
///   that `PNPM_CONFIG_SCOPE=` yields an unscoped `pnpm login`. Dropping it
///   would let the lower layer's scope leak through, diverging from the
///   TypeScript CLI.
fn read_env_allow_empty<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper).or_else(|| Sys::var(&lower))
}

/// Parse `value` as JSON. Returns `None` on parse failure so the
/// caller falls through to its default (skip the field).
fn parse_json<Target: DeserializeOwned>(value: &str) -> Option<Target> {
    serde_json::from_str(value).ok()
}

/// Parse `value` as JSON; if that fails, retry with `value` wrapped as
/// a JSON string. Used for enum fields whose serde representation is a
/// bare identifier (`hoisted`, `warn-only`, `no-downgrade`, ...) — the
/// raw env var value isn't valid JSON on its own but becomes valid
/// once quoted.
fn parse_json_or_string<Target: DeserializeOwned>(value: &str) -> Option<Target> {
    parse_json(value).or_else(|| {
        let quoted = serde_json::to_string(value).ok()?;
        parse_json(&quoted)
    })
}

/// Parse a `hoist_pattern` / `public_hoist_pattern` env var into the
/// tri-state `Option<Option<Vec<String>>>` shape used by
/// [`WorkspaceSettings`].
///
/// Env vars cannot express the "explicit null disable" state that yaml
/// supports: an array-schema env var is JSON-parsed and then required to
/// be an array, so `PNPM_CONFIG_HOIST_PATTERN=null` fails the array check
/// and is silently dropped. The tri-state's `Some(None)` branch stays
/// reachable through yaml only; from env we either return `None` (parse
/// failed, leave config default) or `Some(Some(vec))` (explicit list).
fn parse_tri_array(value: &str) -> Option<Option<Vec<String>>> {
    parse_json::<Vec<String>>(value).map(Some)
}

macro_rules! json_field {
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix)
            && let Some(v) = parse_json(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! string_field {
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix) {
            $settings.$field = Some(s);
        }
    };
}
macro_rules! enum_field {
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal, $ty:ty) => {
        if let Some(s) = read_env::<$sys>($suffix)
            && let Some(v) = parse_json_or_string::<$ty>(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! tri_array_field {
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix)
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
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix) {
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
        let mut settings = WorkspaceSettings::default();

        settings.read_general_env::<Sys>();
        settings.read_layout_env::<Sys>();
        settings.read_lockfile_env::<Sys>();
        settings.read_registry_env::<Sys>();
        settings.read_resolution_env::<Sys>();
        settings.read_network_env::<Sys>();
        settings.read_scripts_env::<Sys>();
        settings.read_workspace_env::<Sys>();
        settings.read_trust_env::<Sys>();
        settings.read_init_env::<Sys>();
        settings.read_verification_env::<Sys>();
        settings.read_save_env::<Sys>();

        settings
    }
    fn read_general_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, bail, "BAIL");
        json_field!(settings, Sys, ci, "CI");
        json_field!(settings, Sys, update_notifier, "UPDATE_NOTIFIER");
        enum_field!(settings, Sys, color, "COLOR", ColorMode);
        json_field!(settings, Sys, embed_readme, "EMBED_README");
        json_field!(settings, Sys, ignore_pnpmfile, "IGNORE_PNPMFILE");
        json_field!(settings, Sys, ignore_workspace_root_check, "IGNORE_WORKSPACE_ROOT_CHECK");
        json_field!(settings, Sys, optional, "OPTIONAL");
        json_field!(settings, Sys, package_lock, "PACKAGE_LOCK");
        json_field!(settings, Sys, pending, "PENDING");
        json_field!(settings, Sys, recursive_install, "RECURSIVE_INSTALL");
        json_field!(settings, Sys, reverse, "REVERSE");
        json_field!(settings, Sys, stream, "STREAM");
        json_field!(settings, Sys, aggregate_output, "AGGREGATE_OUTPUT");
        json_field!(settings, Sys, reporter_hide_prefix, "REPORTER_HIDE_PREFIX");
        json_field!(settings, Sys, use_stderr, "USE_STDERR");
        json_field!(settings, Sys, ignore_workspace, "IGNORE_WORKSPACE");
        json_field!(settings, Sys, shell_emulator, "SHELL_EMULATOR");
        json_field!(settings, Sys, skip_manifest_obfuscation, "SKIP_MANIFEST_OBFUSCATION");
        json_field!(settings, Sys, sort, "SORT");
        json_field!(settings, Sys, use_beta_cli, "USE_BETA_CLI");
    }

    fn read_layout_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, hoist, "HOIST");
        tri_array_field!(settings, Sys, hoist_pattern, "HOIST_PATTERN");
        tri_array_field!(settings, Sys, public_hoist_pattern, "PUBLIC_HOIST_PATTERN");
        json_field!(settings, Sys, shamefully_hoist, "SHAMEFULLY_HOIST");
        string_field!(settings, Sys, store_dir, "STORE_DIR");
        string_field!(settings, Sys, state_dir, "STATE_DIR");
        string_field!(settings, Sys, modules_dir, "MODULES_DIR");
        enum_field!(settings, Sys, node_linker, "NODE_LINKER", NodeLinker);
        json_field!(settings, Sys, node_experimental_package_map, "NODE_EXPERIMENTAL_PACKAGE_MAP");
        enum_field!(
            settings,
            Sys,
            node_package_map_type,
            "NODE_PACKAGE_MAP_TYPE",
            NodePackageMapType
        );
        json_field!(settings, Sys, symlink, "SYMLINK");
        string_field!(settings, Sys, virtual_store_dir, "VIRTUAL_STORE_DIR");
        enum_field!(settings, Sys, virtual_store_type, "VIRTUAL_STORE_TYPE", VirtualStoreType);
        json_field!(settings, Sys, enable_global_virtual_store, "ENABLE_GLOBAL_VIRTUAL_STORE");
        json_field!(settings, Sys, virtual_store_only, "VIRTUAL_STORE_ONLY");
        json_field!(settings, Sys, enable_modules_dir, "ENABLE_MODULES_DIR");
        json_field!(settings, Sys, global_shims, "GLOBAL_SHIMS");
        string_field!(settings, Sys, global_virtual_store_dir, "GLOBAL_VIRTUAL_STORE_DIR");
        string_field!(settings, Sys, global_dir, "GLOBAL_DIR");
        string_field!(settings, Sys, global_bin_dir, "GLOBAL_BIN_DIR");
        enum_field!(
            settings,
            Sys,
            package_import_method,
            "PACKAGE_IMPORT_METHOD",
            PackageImportMethod
        );
        json_field!(settings, Sys, modules_cache_max_age, "MODULES_CACHE_MAX_AGE");
        json_field!(settings, Sys, virtual_store_dir_max_length, "VIRTUAL_STORE_DIR_MAX_LENGTH");
        json_field!(settings, Sys, peers_suffix_max_length, "PEERS_SUFFIX_MAX_LENGTH");
    }

    fn read_lockfile_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, lockfile, "LOCKFILE");
        string_field!(settings, Sys, lockfile_dir, "LOCKFILE_DIR");
        json_field!(settings, Sys, prefer_frozen_lockfile, "PREFER_FROZEN_LOCKFILE");
        json_field!(settings, Sys, prefer_symlinked_executables, "PREFER_SYMLINKED_EXECUTABLES");
        json_field!(settings, Sys, frozen_lockfile, "FROZEN_LOCKFILE");
        json_field!(settings, Sys, deploy_all_files, "DEPLOY_ALL_FILES");
        json_field!(settings, Sys, force_legacy_deploy, "FORCE_LEGACY_DEPLOY");
        json_field!(settings, Sys, shared_workspace_lockfile, "SHARED_WORKSPACE_LOCKFILE");
        json_field!(settings, Sys, git_branch_lockfile, "GIT_BRANCH_LOCKFILE");
        json_field!(settings, Sys, merge_git_branch_lockfiles, "MERGE_GIT_BRANCH_LOCKFILES");
        json_field!(
            settings,
            Sys,
            merge_git_branch_lockfiles_branch_pattern,
            "MERGE_GIT_BRANCH_LOCKFILES_BRANCH_PATTERN"
        );
    }

    fn read_registry_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, offline, "OFFLINE");
        json_field!(settings, Sys, prefer_offline, "PREFER_OFFLINE");
        json_field!(settings, Sys, lockfile_include_tarball_url, "LOCKFILE_INCLUDE_TARBALL_URL");
        string_field!(settings, Sys, registry, "REGISTRY");
        // Unlike its `string_field!` neighbors, `scope` keeps an empty value;
        // see [`read_env_allow_empty`] for why.
        if let Some(scope) = read_env_allow_empty::<Sys>("SCOPE") {
            settings.scope = Some(scope);
        }
        string_field!(settings, Sys, pnpr_server, "PNPR_SERVER");
        string_field!(settings, Sys, https_proxy, "HTTPS_PROXY");
        string_field!(settings, Sys, http_proxy, "HTTP_PROXY");
        string_field!(settings, Sys, proxy, "PROXY");
        if let Some(value) = read_env::<Sys>("NO_PROXY") {
            settings.no_proxy = Some(serde_json::Value::String(value));
        }
        if let Some(value) = read_env::<Sys>("NOPROXY") {
            settings.noproxy = Some(serde_json::Value::String(value));
        }
    }

    fn read_resolution_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, auto_install_peers, "AUTO_INSTALL_PEERS");
        json_field!(
            settings,
            Sys,
            auto_install_peers_from_highest_match,
            "AUTO_INSTALL_PEERS_FROM_HIGHEST_MATCH"
        );
        json_field!(settings, Sys, exclude_links_from_lockfile, "EXCLUDE_LINKS_FROM_LOCKFILE");
        json_field!(settings, Sys, hoist_workspace_packages, "HOIST_WORKSPACE_PACKAGES");
        enum_field!(settings, Sys, hoisting_limits, "HOISTING_LIMITS", HoistingLimits);
        json_field!(settings, Sys, external_dependencies, "EXTERNAL_DEPENDENCIES");
        json_field!(settings, Sys, dedupe_peer_dependents, "DEDUPE_PEER_DEPENDENTS");
        json_field!(settings, Sys, dedupe_peers, "DEDUPE_PEERS");
        json_field!(settings, Sys, dedupe_direct_deps, "DEDUPE_DIRECT_DEPS");
        json_field!(settings, Sys, prefer_workspace_packages, "PREFER_WORKSPACE_PACKAGES");
        json_field!(settings, Sys, dedupe_injected_deps, "DEDUPE_INJECTED_DEPS");
        json_field!(settings, Sys, strict_peer_dependencies, "STRICT_PEER_DEPENDENCIES");
        json_field!(settings, Sys, ignore_compatibility_db, "IGNORE_COMPATIBILITY_DB");
        json_field!(
            settings,
            Sys,
            resolve_peers_from_workspace_root,
            "RESOLVE_PEERS_FROM_WORKSPACE_ROOT"
        );
        json_field!(settings, Sys, block_exotic_subdeps, "BLOCK_EXOTIC_SUBDEPS");
        json_field!(settings, Sys, verify_store_integrity, "VERIFY_STORE_INTEGRITY");
        json_field!(
            settings,
            Sys,
            strict_store_pkg_content_check,
            "STRICT_STORE_PKG_CONTENT_CHECK"
        );
        json_field!(settings, Sys, include_workspace_root, "INCLUDE_WORKSPACE_ROOT");
        json_field!(settings, Sys, ignore_workspace_cycles, "IGNORE_WORKSPACE_CYCLES");
        json_field!(settings, Sys, disallow_workspace_cycles, "DISALLOW_WORKSPACE_CYCLES");
        json_field!(settings, Sys, side_effects_cache, "SIDE_EFFECTS_CACHE");
        json_field!(settings, Sys, side_effects_cache_readonly, "SIDE_EFFECTS_CACHE_READONLY");
    }

    fn read_network_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, fetch_retries, "FETCH_RETRIES");
        json_field!(settings, Sys, fetch_retry_factor, "FETCH_RETRY_FACTOR");
        json_field!(settings, Sys, fetch_retry_mintimeout, "FETCH_RETRY_MINTIMEOUT");
        json_field!(settings, Sys, fetch_retry_maxtimeout, "FETCH_RETRY_MAXTIMEOUT");
        json_field!(settings, Sys, network_concurrency, "NETWORK_CONCURRENCY");
        json_field!(settings, Sys, maxsockets, "MAXSOCKETS");
        json_field!(settings, Sys, max_sockets, "MAX_SOCKETS");
        json_field!(settings, Sys, fetch_timeout, "FETCH_TIMEOUT");
        json_field!(settings, Sys, fetch_warn_timeout_ms, "FETCH_WARN_TIMEOUT_MS");
        json_field!(settings, Sys, fetch_min_speed_ki_bps, "FETCH_MIN_SPEED_KI_BPS");
        string_field!(settings, Sys, user_agent, "USER_AGENT");
    }

    fn read_scripts_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, patched_dependencies, "PATCHED_DEPENDENCIES");
        json_field!(settings, Sys, allow_unused_patches, "ALLOW_UNUSED_PATCHES");
        string_field!(settings, Sys, patches_dir, "PATCHES_DIR");
        string_field!(settings, Sys, global_pnpmfile, "GLOBAL_PNPMFILE");
        enum_field!(settings, Sys, pnpmfile, "PNPMFILE", crate::PnpmfileSetting);
        json_field!(settings, Sys, allow_builds, "ALLOW_BUILDS");
        json_field!(settings, Sys, dangerously_allow_all_builds, "DANGEROUSLY_ALLOW_ALL_BUILDS");
        json_field!(settings, Sys, strict_dep_builds, "STRICT_DEP_BUILDS");
        json_field!(settings, Sys, ignore_scripts, "IGNORE_SCRIPTS");
        json_field!(settings, Sys, git_checks, "GIT_CHECKS");
        json_field!(settings, Sys, engine_strict, "ENGINE_STRICT");
        string_field!(settings, Sys, node_version, "NODE_VERSION");
        enum_field!(settings, Sys, runtime_on_fail, "RUNTIME_ON_FAIL", RuntimeOnFail);
        json_field!(settings, Sys, node_download_mirrors, "NODE_DOWNLOAD_MIRRORS");
        enum_field!(
            settings,
            Sys,
            scripts_prepend_node_path,
            "SCRIPTS_PREPEND_NODE_PATH",
            ScriptsPrependNodePath
        );
        json_field!(settings, Sys, enable_pre_post_scripts, "ENABLE_PRE_POST_SCRIPTS");
        tri_string_field!(settings, Sys, script_shell, "SCRIPT_SHELL");
        tri_string_field!(settings, Sys, node_options, "NODE_OPTIONS");
        json_field!(settings, Sys, unsafe_perm, "UNSAFE_PERM");
        json_field!(settings, Sys, child_concurrency, "CHILD_CONCURRENCY");
        json_field!(settings, Sys, workspace_concurrency, "WORKSPACE_CONCURRENCY");
        json_field!(settings, Sys, git_shallow_hosts, "GIT_SHALLOW_HOSTS");
    }

    fn read_workspace_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, test_pattern, "TEST_PATTERN");
        json_field!(settings, Sys, legacy_dir_filtering, "LEGACY_DIR_FILTERING");
        json_field!(
            settings,
            Sys,
            sync_injected_deps_after_scripts,
            "SYNC_INJECTED_DEPS_AFTER_SCRIPTS"
        );
        json_field!(settings, Sys, changed_files_ignore_pattern, "CHANGED_FILES_IGNORE_PATTERN");
        json_field!(settings, Sys, supported_architectures, "SUPPORTED_ARCHITECTURES");
        json_field!(settings, Sys, ignored_optional_dependencies, "IGNORED_OPTIONAL_DEPENDENCIES");
        json_field!(settings, Sys, overrides, "OVERRIDES");
        json_field!(settings, Sys, package_extensions, "PACKAGE_EXTENSIONS");
    }

    fn read_trust_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        string_field!(settings, Sys, cache_dir, "CACHE_DIR");
        json_field!(settings, Sys, dlx_cache_max_age, "DLX_CACHE_MAX_AGE");
        json_field!(settings, Sys, minimum_release_age, "MINIMUM_RELEASE_AGE");
        json_field!(settings, Sys, minimum_release_age_exclude, "MINIMUM_RELEASE_AGE_EXCLUDE");
        json_field!(
            settings,
            Sys,
            minimum_release_age_ignore_missing_time,
            "MINIMUM_RELEASE_AGE_IGNORE_MISSING_TIME"
        );
        json_field!(settings, Sys, minimum_release_age_strict, "MINIMUM_RELEASE_AGE_STRICT");
        json_field!(settings, Sys, trust_lockfile, "TRUST_LOCKFILE");
        enum_field!(settings, Sys, trust_policy, "TRUST_POLICY", TrustPolicy);
        enum_field!(settings, Sys, pm_on_fail, "PM_ON_FAIL", PmOnFail);
    }

    fn read_init_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, init_package_manager, "INIT_PACKAGE_MANAGER");
        enum_field!(settings, Sys, init_type, "INIT_TYPE", InitType);
        string_field!(settings, Sys, init_author_name, "INIT_AUTHOR_NAME");
        string_field!(settings, Sys, init_author_email, "INIT_AUTHOR_EMAIL");
        string_field!(settings, Sys, init_author_url, "INIT_AUTHOR_URL");
        string_field!(settings, Sys, init_license, "INIT_LICENSE");
        string_field!(settings, Sys, init_version, "INIT_VERSION");
    }

    fn read_verification_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        // pnpm applies this env var on presence alone (`!= null`) and
        // assigns the raw value without validation, so presence always
        // overrides the other config layers: an empty value assigns an
        // empty string — falsy there, the gate is off — and an
        // unrecognized value is truthy — the check runs but matches no
        // action. `read_env` filters empty values, so read the var
        // directly.
        if let Some(s) = Sys::var("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN")
            .or_else(|| Sys::var("pnpm_config_verify_deps_before_run"))
        {
            settings.verify_deps_before_run = Some(if s.is_empty() {
                VerifyDepsBeforeRun::False
            } else {
                parse_json_or_string::<VerifyDepsBeforeRun>(&s).unwrap_or(VerifyDepsBeforeRun::True)
            });
        }
    }

    fn read_save_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        enum_field!(settings, Sys, audit_level, "AUDIT_LEVEL", AuditLevel);
        json_field!(settings, Sys, audit_config, "AUDIT_CONFIG");
        json_field!(settings, Sys, trust_policy_exclude, "TRUST_POLICY_EXCLUDE");
        json_field!(settings, Sys, trust_policy_ignore_after, "TRUST_POLICY_IGNORE_AFTER");
        enum_field!(settings, Sys, resolution_mode, "RESOLUTION_MODE", ResolutionMode);
        enum_field!(settings, Sys, catalog_mode, "CATALOG_MODE", CatalogMode);
        string_field!(settings, Sys, save_catalog_name, "SAVE_CATALOG_NAME");
        if let Some(save_prefix) = read_env_allow_empty::<Sys>("SAVE_PREFIX") {
            settings.save_prefix = Some(save_prefix);
        }
        json_field!(settings, Sys, save_exact, "SAVE_EXACT");
        json_field!(settings, Sys, save_peer, "SAVE_PEER");
        enum_field!(
            settings,
            Sys,
            save_workspace_protocol,
            "SAVE_WORKSPACE_PROTOCOL",
            SaveWorkspaceProtocol
        );
        json_field!(settings, Sys, registry_supports_time_field, "REGISTRY_SUPPORTS_TIME_FIELD");
        json_field!(settings, Sys, allowed_deprecated_versions, "ALLOWED_DEPRECATED_VERSIONS");
        json_field!(settings, Sys, update_config, "UPDATE_CONFIG");
        json_field!(settings, Sys, peer_dependency_rules, "PEER_DEPENDENCY_RULES");
    }
}

#[cfg(test)]
mod tests;
