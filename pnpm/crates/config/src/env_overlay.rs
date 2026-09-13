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

#[macro_use]
mod fields;
mod parsing;

use crate::{
    AuditLevel, CatalogMode, ColorMode, HoistingLimits, InitType, NodeLinker, NodePackageMapType,
    PackageImportMethod, PmOnFail, ResolutionMode, RuntimeOnFail, SaveWorkspaceProtocol,
    ScriptsPrependNodePath, TrustPolicy, VerifyDepsBeforeRun, VirtualStoreType, WorkspaceSettings,
    api::EnvVar,
};
use parsing::{parse_json, parse_json_or_string, parse_tri_array, read_env, read_env_allow_empty};

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
        json_field!(settings, Sys;
            bail => "BAIL",
            ci => "CI",
            update_notifier => "UPDATE_NOTIFIER",
        );
        enum_field!(settings, Sys, color, "COLOR", ColorMode);
        json_field!(settings, Sys;
            embed_readme => "EMBED_README",
            ignore_pnpmfile => "IGNORE_PNPMFILE",
            ignore_workspace_root_check => "IGNORE_WORKSPACE_ROOT_CHECK",
            optional => "OPTIONAL",
            package_lock => "PACKAGE_LOCK",
            pending => "PENDING",
            recursive_install => "RECURSIVE_INSTALL",
            reverse => "REVERSE",
            stream => "STREAM",
            aggregate_output => "AGGREGATE_OUTPUT",
            reporter_hide_prefix => "REPORTER_HIDE_PREFIX",
            use_stderr => "USE_STDERR",
            ignore_workspace => "IGNORE_WORKSPACE",
            shell_emulator => "SHELL_EMULATOR",
            skip_manifest_obfuscation => "SKIP_MANIFEST_OBFUSCATION",
            sort => "SORT",
            use_beta_cli => "USE_BETA_CLI",
        );
    }

    fn read_layout_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, hoist, "HOIST");
        tri_array_field!(settings, Sys;
            hoist_pattern => "HOIST_PATTERN",
            public_hoist_pattern => "PUBLIC_HOIST_PATTERN",
        );
        json_field!(settings, Sys, shamefully_hoist, "SHAMEFULLY_HOIST");
        string_field!(settings, Sys;
            store_dir => "STORE_DIR",
            state_dir => "STATE_DIR",
            modules_dir => "MODULES_DIR",
        );
        enum_field!(settings, Sys, node_linker, "NODE_LINKER", NodeLinker);
        json_field!(
            settings,
            Sys,
            node_experimental_package_map,
            "NODE_EXPERIMENTAL_PACKAGE_MAP"
        );
        enum_field!(
            settings,
            Sys,
            node_package_map_type,
            "NODE_PACKAGE_MAP_TYPE",
            NodePackageMapType
        );
        settings.read_store_env::<Sys>();
    }

    fn read_store_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, symlink, "SYMLINK");
        string_field!(settings, Sys, virtual_store_dir, "VIRTUAL_STORE_DIR");
        enum_field!(
            settings,
            Sys,
            virtual_store_type,
            "VIRTUAL_STORE_TYPE",
            VirtualStoreType
        );
        json_field!(settings, Sys;
            enable_global_virtual_store => "ENABLE_GLOBAL_VIRTUAL_STORE",
            virtual_store_only => "VIRTUAL_STORE_ONLY",
            enable_modules_dir => "ENABLE_MODULES_DIR",
            global_shims => "GLOBAL_SHIMS",
        );
        string_field!(settings, Sys;
            global_virtual_store_dir => "GLOBAL_VIRTUAL_STORE_DIR",
            global_dir => "GLOBAL_DIR",
            global_bin_dir => "GLOBAL_BIN_DIR",
        );
        enum_field!(
            settings,
            Sys,
            package_import_method,
            "PACKAGE_IMPORT_METHOD",
            PackageImportMethod
        );
        json_field!(settings, Sys;
            modules_cache_max_age => "MODULES_CACHE_MAX_AGE",
            virtual_store_dir_max_length => "VIRTUAL_STORE_DIR_MAX_LENGTH",
            peers_suffix_max_length => "PEERS_SUFFIX_MAX_LENGTH",
        );
    }

    fn read_lockfile_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, lockfile, "LOCKFILE");
        string_field!(settings, Sys, lockfile_dir, "LOCKFILE_DIR");
        json_field!(settings, Sys;
            prefer_frozen_lockfile => "PREFER_FROZEN_LOCKFILE",
            prefer_symlinked_executables => "PREFER_SYMLINKED_EXECUTABLES",
            frozen_lockfile => "FROZEN_LOCKFILE",
            deploy_all_files => "DEPLOY_ALL_FILES",
            force_legacy_deploy => "FORCE_LEGACY_DEPLOY",
            shared_workspace_lockfile => "SHARED_WORKSPACE_LOCKFILE",
            git_branch_lockfile => "GIT_BRANCH_LOCKFILE",
            merge_git_branch_lockfiles => "MERGE_GIT_BRANCH_LOCKFILES",
            merge_git_branch_lockfiles_branch_pattern => "MERGE_GIT_BRANCH_LOCKFILES_BRANCH_PATTERN",
        );
    }

    fn read_registry_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys;
            offline => "OFFLINE",
            prefer_offline => "PREFER_OFFLINE",
            lockfile_include_tarball_url => "LOCKFILE_INCLUDE_TARBALL_URL",
        );
        string_field!(settings, Sys, registry, "REGISTRY");
        // Unlike its `string_field!` neighbors, `scope` keeps an empty value;
        // see [`read_env_allow_empty`] for why.
        if let Some(scope) = read_env_allow_empty::<Sys>("SCOPE") {
            settings.scope = Some(scope);
        }
        string_field!(settings, Sys;
            pnpr_server => "PNPR_SERVER",
            https_proxy => "HTTPS_PROXY",
            http_proxy => "HTTP_PROXY",
            proxy => "PROXY",
        );
        if let Some(value) = read_env::<Sys>("NO_PROXY") {
            settings.no_proxy = Some(serde_json::Value::String(value));
        }
        if let Some(value) = read_env::<Sys>("NOPROXY") {
            settings.noproxy = Some(serde_json::Value::String(value));
        }
    }

    fn read_resolution_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys;
            auto_install_peers => "AUTO_INSTALL_PEERS",
            auto_install_peers_from_highest_match => "AUTO_INSTALL_PEERS_FROM_HIGHEST_MATCH",
            exclude_links_from_lockfile => "EXCLUDE_LINKS_FROM_LOCKFILE",
            hoist_workspace_packages => "HOIST_WORKSPACE_PACKAGES",
        );
        enum_field!(
            settings,
            Sys,
            hoisting_limits,
            "HOISTING_LIMITS",
            HoistingLimits
        );
        json_field!(settings, Sys;
            external_dependencies => "EXTERNAL_DEPENDENCIES",
            dedupe_peer_dependents => "DEDUPE_PEER_DEPENDENTS",
            dedupe_peers => "DEDUPE_PEERS",
            dedupe_direct_deps => "DEDUPE_DIRECT_DEPS",
            prefer_workspace_packages => "PREFER_WORKSPACE_PACKAGES",
            dedupe_injected_deps => "DEDUPE_INJECTED_DEPS",
            strict_peer_dependencies => "STRICT_PEER_DEPENDENCIES",
            ignore_compatibility_db => "IGNORE_COMPATIBILITY_DB",
            resolve_peers_from_workspace_root => "RESOLVE_PEERS_FROM_WORKSPACE_ROOT",
            block_exotic_subdeps => "BLOCK_EXOTIC_SUBDEPS",
            verify_store_integrity => "VERIFY_STORE_INTEGRITY",
            strict_store_pkg_content_check => "STRICT_STORE_PKG_CONTENT_CHECK",
            include_workspace_root => "INCLUDE_WORKSPACE_ROOT",
            ignore_workspace_cycles => "IGNORE_WORKSPACE_CYCLES",
            disallow_workspace_cycles => "DISALLOW_WORKSPACE_CYCLES",
            side_effects_cache => "SIDE_EFFECTS_CACHE",
            side_effects_cache_readonly => "SIDE_EFFECTS_CACHE_READONLY",
        );
    }

    fn read_network_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys;
            fetch_retries => "FETCH_RETRIES",
            fetch_retry_factor => "FETCH_RETRY_FACTOR",
            fetch_retry_mintimeout => "FETCH_RETRY_MINTIMEOUT",
            fetch_retry_maxtimeout => "FETCH_RETRY_MAXTIMEOUT",
            network_concurrency => "NETWORK_CONCURRENCY",
            maxsockets => "MAXSOCKETS",
            max_sockets => "MAX_SOCKETS",
            fetch_timeout => "FETCH_TIMEOUT",
            fetch_warn_timeout_ms => "FETCH_WARN_TIMEOUT_MS",
            fetch_min_speed_ki_bps => "FETCH_MIN_SPEED_KI_BPS",
        );
        string_field!(settings, Sys, user_agent, "USER_AGENT");
    }

    fn read_scripts_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys;
            patched_dependencies => "PATCHED_DEPENDENCIES",
            allow_unused_patches => "ALLOW_UNUSED_PATCHES",
        );
        string_field!(settings, Sys;
            patches_dir => "PATCHES_DIR",
            global_pnpmfile => "GLOBAL_PNPMFILE",
        );
        enum_field!(settings, Sys, pnpmfile, "PNPMFILE", crate::PnpmfileSetting);
        json_field!(settings, Sys;
            allow_builds => "ALLOW_BUILDS",
            dangerously_allow_all_builds => "DANGEROUSLY_ALLOW_ALL_BUILDS",
            strict_dep_builds => "STRICT_DEP_BUILDS",
            ignore_scripts => "IGNORE_SCRIPTS",
            git_checks => "GIT_CHECKS",
            engine_strict => "ENGINE_STRICT",
        );
        settings.read_runtime_env::<Sys>();
    }

    fn read_runtime_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        string_field!(settings, Sys, node_version, "NODE_VERSION");
        enum_field!(
            settings,
            Sys,
            runtime_on_fail,
            "RUNTIME_ON_FAIL",
            RuntimeOnFail
        );
        json_field!(
            settings,
            Sys,
            node_download_mirrors,
            "NODE_DOWNLOAD_MIRRORS"
        );
        enum_field!(
            settings,
            Sys,
            scripts_prepend_node_path,
            "SCRIPTS_PREPEND_NODE_PATH",
            ScriptsPrependNodePath
        );
        json_field!(
            settings,
            Sys,
            enable_pre_post_scripts,
            "ENABLE_PRE_POST_SCRIPTS"
        );
        tri_string_field!(settings, Sys;
            script_shell => "SCRIPT_SHELL",
            node_options => "NODE_OPTIONS",
        );
        json_field!(settings, Sys;
            unsafe_perm => "UNSAFE_PERM",
            child_concurrency => "CHILD_CONCURRENCY",
            workspace_concurrency => "WORKSPACE_CONCURRENCY",
            git_shallow_hosts => "GIT_SHALLOW_HOSTS",
        );
    }

    fn read_workspace_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys;
            test_pattern => "TEST_PATTERN",
            legacy_dir_filtering => "LEGACY_DIR_FILTERING",
            sync_injected_deps_after_scripts => "SYNC_INJECTED_DEPS_AFTER_SCRIPTS",
            changed_files_ignore_pattern => "CHANGED_FILES_IGNORE_PATTERN",
            supported_architectures => "SUPPORTED_ARCHITECTURES",
            ignored_optional_dependencies => "IGNORED_OPTIONAL_DEPENDENCIES",
            overrides => "OVERRIDES",
            package_extensions => "PACKAGE_EXTENSIONS",
        );
    }

    fn read_trust_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        string_field!(settings, Sys, cache_dir, "CACHE_DIR");
        json_field!(settings, Sys;
            dlx_cache_max_age => "DLX_CACHE_MAX_AGE",
            minimum_release_age => "MINIMUM_RELEASE_AGE",
            minimum_release_age_exclude => "MINIMUM_RELEASE_AGE_EXCLUDE",
            minimum_release_age_ignore_missing_time => "MINIMUM_RELEASE_AGE_IGNORE_MISSING_TIME",
            minimum_release_age_strict => "MINIMUM_RELEASE_AGE_STRICT",
            trust_lockfile => "TRUST_LOCKFILE",
        );
        enum_field!(settings, Sys;
            trust_policy => "TRUST_POLICY", TrustPolicy,
            pm_on_fail => "PM_ON_FAIL", PmOnFail,
        );
    }

    fn read_init_env<Sys: EnvVar>(&mut self) {
        let settings = self;
        json_field!(settings, Sys, init_package_manager, "INIT_PACKAGE_MANAGER");
        enum_field!(settings, Sys, init_type, "INIT_TYPE", InitType);
        string_field!(settings, Sys;
            init_author_name => "INIT_AUTHOR_NAME",
            init_author_email => "INIT_AUTHOR_EMAIL",
            init_author_url => "INIT_AUTHOR_URL",
            init_license => "INIT_LICENSE",
            init_version => "INIT_VERSION",
        );
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
        json_field!(settings, Sys;
            audit_config => "AUDIT_CONFIG",
            trust_policy_exclude => "TRUST_POLICY_EXCLUDE",
            trust_policy_ignore_after => "TRUST_POLICY_IGNORE_AFTER",
        );
        enum_field!(settings, Sys;
            resolution_mode => "RESOLUTION_MODE", ResolutionMode,
            catalog_mode => "CATALOG_MODE", CatalogMode,
        );
        string_field!(settings, Sys, save_catalog_name, "SAVE_CATALOG_NAME");
        if let Some(save_prefix) = read_env_allow_empty::<Sys>("SAVE_PREFIX") {
            settings.save_prefix = Some(save_prefix);
        }
        json_field!(settings, Sys;
            save_exact => "SAVE_EXACT",
            save_peer => "SAVE_PEER",
        );
        enum_field!(
            settings,
            Sys,
            save_workspace_protocol,
            "SAVE_WORKSPACE_PROTOCOL",
            SaveWorkspaceProtocol
        );
        json_field!(settings, Sys;
            registry_supports_time_field => "REGISTRY_SUPPORTS_TIME_FIELD",
            allowed_deprecated_versions => "ALLOWED_DEPRECATED_VERSIONS",
            update_config => "UPDATE_CONFIG",
            peer_dependency_rules => "PEER_DEPENDENCY_RULES",
        );
    }
}

#[cfg(test)]
mod tests;
