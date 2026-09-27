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
    AuditLevel, CatalogMode, ColorMode, HoistingLimits, InitType, LogLevel, NodeLinker,
    NodePackageMapType, PackageImportMethod, PmOnFail, ReporterType, ResolutionMode, RuntimeOnFail,
    SaveWorkspaceProtocol, ScriptsPrependNodePath, TrustPolicy, VerifyDepsBeforeRun,
    VirtualStoreType, WorkspaceSettings,
    api::EnvVar,
    naming_cases::{to_camel_case, to_kebab_case},
};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

/// Read an env var by suffix, accepting both `PNPM_CONFIG_<UPPER>` and
/// `pnpm_config_<lower>`. Empty values are treated as unset.
fn read_env<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper)
        .or_else(|| Sys::var(&lower))
        .filter(|value| !value.is_empty())
}

/// Read an env var by suffix, keeping an empty value as `Some("")`.
///
/// pnpm's own env pass only skips a variable that is absent, never one
/// that is empty, so an empty value clobbers lower-priority layers. For
/// nearly every setting an empty value is indistinguishable from an unset
/// one, which is why [`read_env`] drops it. Three settings are exceptions,
/// where `""` is observably different from unset:
///
/// - `savePrefix`: `""` is the value that selects an exact version pin.
/// - `scope`: `""` must override a scope from the global `config.yaml` so
///   that `PNPM_CONFIG_SCOPE=` yields an unscoped `pnpm login`. Dropping it
///   would let the lower layer's scope leak through, diverging from the
///   TypeScript CLI.
/// - `tagVersionPrefix`: `""` removes the default `"v"` prefix from version
///   tags.
fn read_env_allow_empty<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper).or_else(|| Sys::var(&lower))
}

/// Where a section reader gets one setting's string value.
///
/// The environment is the production source (`WorkspaceSettings::from_pnpm_config_env`,
/// where an empty variable reads as unset); the command line's
/// `--config.<setting>=<value>` tokens are the other
/// (`WorkspaceSettings::from_string_values`). `value_allow_empty` is kept
/// separate because three settings read `""` as a value of their own — see
/// [`read_env_allow_empty`].
#[derive(Clone, Copy)]
struct StringReader<'a> {
    read: &'a dyn Fn(&str) -> Option<String>,
    read_allow_empty: &'a dyn Fn(&str) -> Option<String>,
}

impl StringReader<'_> {
    fn value(&self, suffix: &str) -> Option<String> {
        (self.read)(suffix)
    }

    fn value_allow_empty(&self, suffix: &str) -> Option<String> {
        (self.read_allow_empty)(suffix)
    }
}

/// Answer a field's env-var suffix (`UPDATE_NOTIFIER`) from a dotted
/// `--config.<key>=<value>` map, which names settings by their own spelling.
///
/// The suffix list and the setting names differ only in case and separator, so
/// both spellings a caller can write are tried before the value is given up
/// on; a suffix naming a nested key (`MACOS_BACKUP_EXCLUDE_MODULES_DIR`)
/// matches no dotted setting and reads as unset.
fn dotted_value(values: &BTreeMap<String, String>, suffix: &str) -> Option<String> {
    let lower = suffix.to_lowercase();
    values
        .get(&to_kebab_case(&lower))
        .or_else(|| values.get(&to_camel_case(&lower)))
        .cloned()
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
    parse_json(value)
        .or_else(|| {
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
    ($settings:ident, $r:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $r.value($suffix)
            && let Some(v) = parse_json(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! string_field {
    ($settings:ident, $r:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $r.value($suffix) {
            $settings.$field = Some(s);
        }
    };
}
// Like `string_field!`, but keeps an empty value as `Some("")` instead
// of treating it as unset. For settings where `""` is observably
// different from unset. See [`read_env_allow_empty`].
macro_rules! string_field_allow_empty {
    ($settings:ident, $r:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $r.value_allow_empty($suffix) {
            $settings.$field = Some(s);
        }
    };
}
macro_rules! enum_field {
    ($settings:ident, $r:expr, $field:ident, $suffix:literal, $ty:ty) => {
        if let Some(s) = $r.value($suffix)
            && let Some(v) = parse_json_or_string::<$ty>(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! tri_array_field {
    ($settings:ident, $r:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $r.value($suffix)
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
    ($settings:ident, $r:expr, $field:ident, $suffix:literal) => {
        if let Some(s) = $r.value($suffix) {
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
    /// shape of input as the environment — one string per setting — so they
    /// read through the field list below rather than a second copy of it.
    /// Apply the result through [`Self::apply_to`] after every file layer so
    /// the command line wins, the way `--config.` does upstream.
    #[must_use]
    pub fn from_string_values(values: &BTreeMap<String, String>) -> Self {
        Self::from_reader(&StringReader {
            read: &|suffix| dotted_value(values, suffix).filter(|value| !value.is_empty()),
            read_allow_empty: &|suffix| dotted_value(values, suffix),
        })
    }

    fn from_reader(r: &StringReader<'_>) -> Self {
        let mut settings = WorkspaceSettings::default();

        settings.read_general_env(r);
        settings.read_layout_env(r);
        settings.read_lockfile_env(r);
        settings.read_registry_env(r);
        settings.read_resolution_env(r);
        settings.read_network_env(r);
        settings.read_scripts_env(r);
        settings.read_workspace_env(r);
        settings.read_trust_env(r);
        settings.read_init_env(r);
        settings.read_verification_env(r);
        settings.read_save_env(r);

        settings
    }
    fn read_general_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, bail, "BAIL");
        json_field!(settings, r, ci, "CI");
        json_field!(settings, r, progress, "PROGRESS");
        json_field!(settings, r, update_notifier, "UPDATE_NOTIFIER");
        enum_field!(settings, r, color, "COLOR", ColorMode);
        enum_field!(settings, r, loglevel, "LOGLEVEL", LogLevel);
        enum_field!(settings, r, reporter, "REPORTER", ReporterType);
        json_field!(settings, r, embed_readme, "EMBED_README");
        json_field!(settings, r, ignore_pnpmfile, "IGNORE_PNPMFILE");
        json_field!(settings, r, ignore_workspace_root_check, "IGNORE_WORKSPACE_ROOT_CHECK");
        json_field!(settings, r, optional, "OPTIONAL");
        json_field!(settings, r, package_lock, "PACKAGE_LOCK");
        json_field!(settings, r, pending, "PENDING");
        json_field!(settings, r, recursive_install, "RECURSIVE_INSTALL");
        json_field!(settings, r, reverse, "REVERSE");
        json_field!(settings, r, stream, "STREAM");
        json_field!(settings, r, aggregate_output, "AGGREGATE_OUTPUT");
        json_field!(settings, r, reporter_hide_prefix, "REPORTER_HIDE_PREFIX");
        json_field!(settings, r, use_stderr, "USE_STDERR");
        json_field!(settings, r, ignore_workspace, "IGNORE_WORKSPACE");
        json_field!(settings, r, shell_emulator, "SHELL_EMULATOR");
        json_field!(settings, r, skip_manifest_obfuscation, "SKIP_MANIFEST_OBFUSCATION");
        json_field!(settings, r, sort, "SORT");
        json_field!(settings, r, use_beta_cli, "USE_BETA_CLI");
    }

    fn read_macos_backup_env(&mut self, r: &StringReader<'_>) {
        if let Some(value) =
            r.value("MACOS_BACKUP_EXCLUDE_MODULES_DIR").and_then(|value| parse_json(&value))
        {
            self.macos_backup.get_or_insert_default().exclude_modules_dir = Some(value);
        }
        if let Some(value) =
            r.value("MACOS_BACKUP_EXCLUDE_STORE_DIR").and_then(|value| parse_json(&value))
        {
            self.macos_backup.get_or_insert_default().exclude_store_dir = Some(value);
        }
    }

    fn read_layout_env(&mut self, r: &StringReader<'_>) {
        self.read_macos_backup_env(r);
        let settings = self;
        json_field!(settings, r, hoist, "HOIST");
        tri_array_field!(settings, r, hoist_pattern, "HOIST_PATTERN");
        tri_array_field!(settings, r, public_hoist_pattern, "PUBLIC_HOIST_PATTERN");
        json_field!(settings, r, shamefully_hoist, "SHAMEFULLY_HOIST");
        string_field!(settings, r, store_dir, "STORE_DIR");
        string_field!(settings, r, state_dir, "STATE_DIR");
        string_field!(settings, r, modules_dir, "MODULES_DIR");
        enum_field!(settings, r, node_linker, "NODE_LINKER", NodeLinker);
        json_field!(settings, r, node_experimental_package_map, "NODE_EXPERIMENTAL_PACKAGE_MAP");
        enum_field!(
            settings,
            r,
            node_package_map_type,
            "NODE_PACKAGE_MAP_TYPE",
            NodePackageMapType
        );
        json_field!(settings, r, symlink, "SYMLINK");
        string_field!(settings, r, virtual_store_dir, "VIRTUAL_STORE_DIR");
        enum_field!(settings, r, virtual_store_type, "VIRTUAL_STORE_TYPE", VirtualStoreType);
        json_field!(settings, r, enable_global_virtual_store, "ENABLE_GLOBAL_VIRTUAL_STORE");
        json_field!(settings, r, virtual_store_only, "VIRTUAL_STORE_ONLY");
        json_field!(settings, r, enable_modules_dir, "ENABLE_MODULES_DIR");
        json_field!(settings, r, global_shims, "GLOBAL_SHIMS");
        string_field!(settings, r, global_virtual_store_dir, "GLOBAL_VIRTUAL_STORE_DIR");
        string_field!(settings, r, global_dir, "GLOBAL_DIR");
        string_field!(settings, r, global_bin_dir, "GLOBAL_BIN_DIR");
        enum_field!(
            settings,
            r,
            package_import_method,
            "PACKAGE_IMPORT_METHOD",
            PackageImportMethod
        );
        json_field!(settings, r, modules_cache_max_age, "MODULES_CACHE_MAX_AGE");
        json_field!(settings, r, virtual_store_dir_max_length, "VIRTUAL_STORE_DIR_MAX_LENGTH");
        json_field!(settings, r, peers_suffix_max_length, "PEERS_SUFFIX_MAX_LENGTH");
    }

    fn read_lockfile_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, lockfile, "LOCKFILE");
        string_field!(settings, r, lockfile_dir, "LOCKFILE_DIR");
        json_field!(settings, r, prefer_frozen_lockfile, "PREFER_FROZEN_LOCKFILE");
        json_field!(settings, r, prefer_symlinked_executables, "PREFER_SYMLINKED_EXECUTABLES");
        json_field!(settings, r, frozen_lockfile, "FROZEN_LOCKFILE");
        json_field!(settings, r, deploy_all_files, "DEPLOY_ALL_FILES");
        json_field!(settings, r, force_legacy_deploy, "FORCE_LEGACY_DEPLOY");
        json_field!(settings, r, shared_workspace_lockfile, "SHARED_WORKSPACE_LOCKFILE");
        json_field!(settings, r, git_branch_lockfile, "GIT_BRANCH_LOCKFILE");
        json_field!(settings, r, merge_git_branch_lockfiles, "MERGE_GIT_BRANCH_LOCKFILES");
        json_field!(
            settings,
            r,
            merge_git_branch_lockfiles_branch_pattern,
            "MERGE_GIT_BRANCH_LOCKFILES_BRANCH_PATTERN"
        );
    }

    fn read_registry_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, offline, "OFFLINE");
        json_field!(settings, r, prefer_offline, "PREFER_OFFLINE");
        json_field!(settings, r, lockfile_include_tarball_url, "LOCKFILE_INCLUDE_TARBALL_URL");
        string_field!(settings, r, registry, "REGISTRY");
        // Unlike its `string_field!` neighbors, `scope` keeps an empty value;
        // see [`read_env_allow_empty`] for why.
        if let Some(scope) = r.value_allow_empty("SCOPE") {
            settings.scope = Some(scope);
        }
        string_field!(settings, r, pnpr_server, "PNPR_SERVER");
        string_field!(settings, r, https_proxy, "HTTPS_PROXY");
        string_field!(settings, r, http_proxy, "HTTP_PROXY");
        string_field!(settings, r, proxy, "PROXY");
        if let Some(value) = r.value("NO_PROXY") {
            settings.no_proxy = Some(serde_json::Value::String(value));
        }
        if let Some(value) = r.value("NOPROXY") {
            settings.noproxy = Some(serde_json::Value::String(value));
        }
    }

    fn read_resolution_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, auto_install_peers, "AUTO_INSTALL_PEERS");
        json_field!(
            settings,
            r,
            auto_install_peers_from_highest_match,
            "AUTO_INSTALL_PEERS_FROM_HIGHEST_MATCH"
        );
        json_field!(settings, r, exclude_links_from_lockfile, "EXCLUDE_LINKS_FROM_LOCKFILE");
        json_field!(settings, r, hoist_workspace_packages, "HOIST_WORKSPACE_PACKAGES");
        enum_field!(settings, r, hoisting_limits, "HOISTING_LIMITS", HoistingLimits);
        json_field!(settings, r, external_dependencies, "EXTERNAL_DEPENDENCIES");
        json_field!(settings, r, dedupe_peer_dependents, "DEDUPE_PEER_DEPENDENTS");
        json_field!(settings, r, dedupe_peers, "DEDUPE_PEERS");
        json_field!(settings, r, auto_dedupe, "AUTO_DEDUPE");
        json_field!(settings, r, dedupe_direct_deps, "DEDUPE_DIRECT_DEPS");
        json_field!(settings, r, prefer_workspace_packages, "PREFER_WORKSPACE_PACKAGES");
        json_field!(settings, r, dedupe_injected_deps, "DEDUPE_INJECTED_DEPS");
        json_field!(settings, r, strict_peer_dependencies, "STRICT_PEER_DEPENDENCIES");
        json_field!(settings, r, ignore_compatibility_db, "IGNORE_COMPATIBILITY_DB");
        json_field!(
            settings,
            r,
            resolve_peers_from_workspace_root,
            "RESOLVE_PEERS_FROM_WORKSPACE_ROOT"
        );
        json_field!(settings, r, block_exotic_subdeps, "BLOCK_EXOTIC_SUBDEPS");
        json_field!(settings, r, verify_store_integrity, "VERIFY_STORE_INTEGRITY");
        json_field!(settings, r, strict_store_pkg_content_check, "STRICT_STORE_PKG_CONTENT_CHECK");
        json_field!(settings, r, include_workspace_root, "INCLUDE_WORKSPACE_ROOT");
        json_field!(settings, r, ignore_workspace_cycles, "IGNORE_WORKSPACE_CYCLES");
        json_field!(settings, r, disallow_workspace_cycles, "DISALLOW_WORKSPACE_CYCLES");
        json_field!(settings, r, side_effects_cache, "SIDE_EFFECTS_CACHE");
        json_field!(settings, r, side_effects_cache_readonly, "SIDE_EFFECTS_CACHE_READONLY");
    }

    fn read_network_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, fetch_retries, "FETCH_RETRIES");
        json_field!(settings, r, fetch_retry_factor, "FETCH_RETRY_FACTOR");
        json_field!(settings, r, fetch_retry_mintimeout, "FETCH_RETRY_MINTIMEOUT");
        json_field!(settings, r, fetch_retry_maxtimeout, "FETCH_RETRY_MAXTIMEOUT");
        json_field!(settings, r, network_concurrency, "NETWORK_CONCURRENCY");
        json_field!(settings, r, maxsockets, "MAXSOCKETS");
        json_field!(settings, r, max_sockets, "MAX_SOCKETS");
        json_field!(settings, r, fetch_timeout, "FETCH_TIMEOUT");
        json_field!(settings, r, fetch_warn_timeout_ms, "FETCH_WARN_TIMEOUT_MS");
        json_field!(settings, r, fetch_min_speed_ki_bps, "FETCH_MIN_SPEED_KI_BPS");
        string_field!(settings, r, user_agent, "USER_AGENT");
    }

    fn read_scripts_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, patched_dependencies, "PATCHED_DEPENDENCIES");
        json_field!(settings, r, allow_unused_patches, "ALLOW_UNUSED_PATCHES");
        string_field!(settings, r, patches_dir, "PATCHES_DIR");
        string_field!(settings, r, global_pnpmfile, "GLOBAL_PNPMFILE");
        enum_field!(settings, r, pnpmfile, "PNPMFILE", crate::PnpmfileSetting);
        json_field!(settings, r, allow_builds, "ALLOW_BUILDS");
        json_field!(settings, r, dangerously_allow_all_builds, "DANGEROUSLY_ALLOW_ALL_BUILDS");
        json_field!(settings, r, strict_dep_builds, "STRICT_DEP_BUILDS");
        json_field!(settings, r, ignore_scripts, "IGNORE_SCRIPTS");
        json_field!(settings, r, git_checks, "GIT_CHECKS");
        json_field!(settings, r, publish_wait_timeout, "PUBLISH_WAIT_TIMEOUT");
        // Empty removes the `v` prefix, so an empty env value must survive.
        string_field_allow_empty!(settings, r, tag_version_prefix, "TAG_VERSION_PREFIX");
        json_field!(settings, r, engine_strict, "ENGINE_STRICT");
        json_field!(settings, r, force_ignores_platform, "FORCE_IGNORES_PLATFORM");
        string_field!(settings, r, node_version, "NODE_VERSION");
        enum_field!(settings, r, runtime_on_fail, "RUNTIME_ON_FAIL", RuntimeOnFail);
        json_field!(settings, r, node_download_mirrors, "NODE_DOWNLOAD_MIRRORS");
        enum_field!(
            settings,
            r,
            scripts_prepend_node_path,
            "SCRIPTS_PREPEND_NODE_PATH",
            ScriptsPrependNodePath
        );
        json_field!(settings, r, enable_pre_post_scripts, "ENABLE_PRE_POST_SCRIPTS");
        tri_string_field!(settings, r, script_shell, "SCRIPT_SHELL");
        tri_string_field!(settings, r, node_options, "NODE_OPTIONS");
        json_field!(settings, r, unsafe_perm, "UNSAFE_PERM");
        json_field!(settings, r, child_concurrency, "CHILD_CONCURRENCY");
        json_field!(settings, r, workspace_concurrency, "WORKSPACE_CONCURRENCY");
        json_field!(settings, r, concurrency_groups, "CONCURRENCY_GROUPS");
        json_field!(settings, r, git_shallow_hosts, "GIT_SHALLOW_HOSTS");
    }

    fn read_workspace_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, test_pattern, "TEST_PATTERN");
        json_field!(settings, r, legacy_dir_filtering, "LEGACY_DIR_FILTERING");
        json_field!(
            settings,
            r,
            sync_injected_deps_after_scripts,
            "SYNC_INJECTED_DEPS_AFTER_SCRIPTS"
        );
        json_field!(settings, r, changed_files_ignore_pattern, "CHANGED_FILES_IGNORE_PATTERN");
        json_field!(settings, r, supported_architectures, "SUPPORTED_ARCHITECTURES");
        json_field!(settings, r, tools, "TOOLS");
        json_field!(settings, r, ignored_optional_dependencies, "IGNORED_OPTIONAL_DEPENDENCIES");
        json_field!(settings, r, overrides, "OVERRIDES");
        json_field!(settings, r, package_extensions, "PACKAGE_EXTENSIONS");
    }

    fn read_trust_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        string_field!(settings, r, cache_dir, "CACHE_DIR");
        json_field!(settings, r, dlx_cache_max_age, "DLX_CACHE_MAX_AGE");
        json_field!(settings, r, minimum_release_age, "MINIMUM_RELEASE_AGE");
        json_field!(settings, r, minimum_release_age_exclude, "MINIMUM_RELEASE_AGE_EXCLUDE");
        json_field!(
            settings,
            r,
            minimum_release_age_ignore_missing_time,
            "MINIMUM_RELEASE_AGE_IGNORE_MISSING_TIME"
        );
        json_field!(settings, r, minimum_release_age_strict, "MINIMUM_RELEASE_AGE_STRICT");
        json_field!(settings, r, trust_lockfile, "TRUST_LOCKFILE");
        enum_field!(settings, r, trust_policy, "TRUST_POLICY", TrustPolicy);
        enum_field!(settings, r, pm_on_fail, "PM_ON_FAIL", PmOnFail);
    }

    fn read_init_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        json_field!(settings, r, init_package_manager, "INIT_PACKAGE_MANAGER");
        enum_field!(settings, r, init_type, "INIT_TYPE", InitType);
        string_field!(settings, r, init_author_name, "INIT_AUTHOR_NAME");
        string_field!(settings, r, init_author_email, "INIT_AUTHOR_EMAIL");
        string_field!(settings, r, init_author_url, "INIT_AUTHOR_URL");
        string_field!(settings, r, init_license, "INIT_LICENSE");
        string_field!(settings, r, init_version, "INIT_VERSION");
    }

    fn read_verification_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        // pnpm applies this env var on presence alone (`!= null`) and
        // assigns the raw value without validation, so presence always
        // overrides the other config layers: an empty value assigns an
        // empty string — falsy there, the gate is off — and an
        // unrecognized value is truthy — the check runs but matches no
        // action. `value` filters empty values, so read the raw one.
        if let Some(s) = r.value_allow_empty("VERIFY_DEPS_BEFORE_RUN") {
            settings.verify_deps_before_run = Some(if s.is_empty() {
                VerifyDepsBeforeRun::False
            } else {
                parse_json_or_string::<VerifyDepsBeforeRun>(&s).unwrap_or(VerifyDepsBeforeRun::True)
            });
        }
    }

    fn read_save_env(&mut self, r: &StringReader<'_>) {
        let settings = self;
        enum_field!(settings, r, audit_level, "AUDIT_LEVEL", AuditLevel);
        json_field!(settings, r, audit_config, "AUDIT_CONFIG");
        json_field!(settings, r, trust_policy_exclude, "TRUST_POLICY_EXCLUDE");
        json_field!(settings, r, trust_policy_ignore_after, "TRUST_POLICY_IGNORE_AFTER");
        enum_field!(settings, r, resolution_mode, "RESOLUTION_MODE", ResolutionMode);
        enum_field!(settings, r, catalog_mode, "CATALOG_MODE", CatalogMode);
        string_field!(settings, r, save_catalog_name, "SAVE_CATALOG_NAME");
        if let Some(save_prefix) = r.value_allow_empty("SAVE_PREFIX") {
            settings.save_prefix = Some(save_prefix);
        }
        json_field!(settings, r, save_exact, "SAVE_EXACT");
        json_field!(settings, r, save_peer, "SAVE_PEER");
        json_field!(settings, r, save_types, "SAVE_TYPES");
        enum_field!(
            settings,
            r,
            save_workspace_protocol,
            "SAVE_WORKSPACE_PROTOCOL",
            SaveWorkspaceProtocol
        );
        json_field!(settings, r, registry_supports_time_field, "REGISTRY_SUPPORTS_TIME_FIELD");
        json_field!(settings, r, allowed_deprecated_versions, "ALLOWED_DEPRECATED_VERSIONS");
        json_field!(settings, r, update_config, "UPDATE_CONFIG");
        json_field!(settings, r, peer_dependency_rules, "PEER_DEPENDENCY_RULES");
    }
}

#[cfg(test)]
mod tests;
