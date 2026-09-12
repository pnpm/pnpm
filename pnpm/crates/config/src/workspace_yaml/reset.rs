use super::{
    Config, EnvVar, GetCurrentDir, GetHomeDir, LinkProbe, Path, WorkspaceSettings,
    explicit_or_default, explicit_pattern, to_camel_case,
};

/// The paths anchored on the lockfile directory, re-anchored the way
/// pinning it does, so a `modulesDir` still set keeps its shape and a
/// pinned lockfile directory keeps its paths.
fn reanchor_lockfile_paths(config: &mut Config, base_dir: &Path) {
    let dir = config.lockfile_dir.clone().unwrap_or_else(|| base_dir.to_path_buf());
    config.anchor_lockfile_paths(&dir);
}

/// `lockfile` follows `packageLock` while nothing sets it, as it does when
/// the config is built.
fn derive_lockfile(config: &mut Config) {
    if !config.explicit_settings.contains_key("lockfile") {
        config.lockfile = config.package_lock;
    }
}

/// The hoist pattern `hoist` allows: the one still set, or the default,
/// and none at all while hoisting is off. A `virtualStoreOnly` install
/// still in force keeps both patterns empty.
fn reset_hoist_pattern(config: &mut Config, defaults: &Config) {
    config.hoist_pattern = config
        .hoist
        .then(|| explicit_or_default(config, "hoistPattern", defaults.hoist_pattern.as_deref()))
        .flatten();
    config.apply_virtual_store_only_derivation();
}

/// The public hoist pattern still set, or the default, with an explicit
/// `shamefullyHoist` overriding it as it does when the config is built,
/// and a `virtualStoreOnly` install still in force keeping it empty.
fn reset_public_hoist_pattern(config: &mut Config, defaults: &Config) {
    config.public_hoist_pattern =
        explicit_or_default(config, "publicHoistPattern", defaults.public_hoist_pattern.as_deref());
    config.apply_shamefully_hoist_derivation();
    config.apply_virtual_store_only_derivation();
}

/// Restore one proxy key and re-resolve the cascade the keys feed.
fn reset_proxy_setting(config: &mut Config, defaults: &Config, key: &str) {
    let (keys, default_keys) = (&mut config.proxy_keys, &defaults.proxy_keys);
    match key {
        "httpsProxy" => keys.https_proxy = default_keys.https_proxy.clone(),
        "httpProxy" => keys.http_proxy = default_keys.http_proxy.clone(),
        "proxy" => keys.legacy_proxy = default_keys.legacy_proxy.clone(),
        "noProxy" => keys.no_proxy = default_keys.no_proxy.clone(),
        _ => keys.noproxy = default_keys.noproxy.clone(),
    }
    config.proxy = config.proxy_keys.resolve();
}

/// Leave `virtualStoreOnly` mode with the hoist patterns the mode
/// snapshotted, a pattern a source explicitly disabled included, unless a
/// source still sets a pattern, and with the derivations that read the
/// patterns re-run.
fn leave_virtual_store_only(config: &mut Config) {
    config.restore_hoist_patterns_after_virtual_store_only();
    if let Some(pattern) = explicit_pattern(config, "hoistPattern") {
        config.hoist_pattern = Some(pattern);
    }
    if !config.hoist {
        config.hoist_pattern = None;
    }
    if let Some(pattern) = explicit_pattern(config, "publicHoistPattern") {
        config.public_hoist_pattern = Some(pattern);
    }
    config.apply_shamefully_hoist_derivation();
}

impl WorkspaceSettings {
    /// Restore the setting `key` names to the value `defaults` holds for
    /// it, for a source that unset the setting. [`Self::apply_to`] has no
    /// value to apply for an unset setting, so this is its counterpart for
    /// deletion. The path settings `apply_to` anchors return to their
    /// default under `base_dir`, and a setting another is derived from
    /// carries the derivation with it. Reports whether `key` named a
    /// setting.
    pub fn reset_setting_to_default<Sys>(
        config: &mut Config,
        defaults: &Config,
        key: &str,
        base_dir: &Path,
    ) -> bool
    where
        Sys: EnvVar + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        type Reset = fn(&mut Config, &Config);
        macro_rules! same_named {
            ($($field:ident),* $(,)?) => {
                vec![$((
                    to_camel_case(stringify!($field)),
                    (|config: &mut Config, defaults: &Config| {
                        config.$field = defaults.$field.clone();
                    }) as Reset,
                ),)*]
            };
        }
        let mut resets: Vec<(String, Reset)> = identically_named_settings!(same_named);
        // The settings both structs name identically but hold at different
        // types, plus the ones `from_resolved` reports only when set.
        resets.extend(same_named! {
            hoist_pattern, public_hoist_pattern, state_dir, lockfile_dir, npmrc_auth_file,
            global_pnpmfile, pnpmfile, global_dir, global_bin_dir, cache_dir,
            prefer_frozen_lockfile, lockfile, merge_git_branch_lockfiles,
            optimistic_repeat_install, minimum_release_age, global_shims, frozen_lockfile,
            registry, scope, pnpr_server, cargo, python, remote_side_effects_cache,
            reporter_hide_prefix, max_sockets, patched_dependencies, patches_dir,
            config_dependencies, dangerously_allow_all_builds, strict_dep_builds,
            ignore_scripts, ignore_pnpmfile, git_checks, engine_strict, node_version,
            runtime_on_fail, node_download_mirrors, scripts_prepend_node_path, script_shell,
            node_options, unsafe_perm, supported_architectures, ignored_optional_dependencies,
            overrides, package_extensions, package_configs, minimum_release_age_exclude,
            minimum_release_age_ignore_missing_time, minimum_release_age_strict,
            trust_lockfile, trust_policy, trust_policy_exclude, trust_policy_exclude_prune,
            trust_policy_ignore_after, init_author_name, init_author_email, init_author_url,
            init_license, init_version, pm_on_fail, versioning, save_catalog_name,
            save_prefix, pipeline_base, child_concurrency, workspace_concurrency, catalogs,
            allow_builds,
        });
        if Self::reset_derived_setting_to_default::<Sys>(config, defaults, key, base_dir) {
            return true;
        }
        let Some((_, reset)) = resets.iter().find(|(name, _)| name == key) else {
            return false;
        };
        reset(config, defaults);
        true
    }

    /// [`Self::reset_setting_to_default`] for the settings another setting
    /// is derived from, the ones [`Config`] holds under another name or
    /// shape, and the deprecated spellings. A derivation reads the settings
    /// still set, so an explicit `hoistPattern` survives `hoist` being
    /// unset, and `lockfile` follows `packageLock` as it does when nothing
    /// set it.
    pub(super) fn reset_derived_setting_to_default<Sys>(
        config: &mut Config,
        defaults: &Config,
        key: &str,
        base_dir: &Path,
    ) -> bool
    where
        Sys: EnvVar + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        match key {
            "storeDir" => config.reset_store_dir_to_default::<Sys>(base_dir),
            "lockfileDir" => {
                config.lockfile_dir = None;
                reanchor_lockfile_paths(config, base_dir);
            }
            "modulesDir" | "virtualStoreDir" => reanchor_lockfile_paths(config, base_dir),
            // Derived from the virtual store directory once that is settled.
            "globalVirtualStoreDir" => {}
            "packageLock" => {
                config.package_lock = defaults.package_lock;
                derive_lockfile(config);
            }
            "lockfile" => derive_lockfile(config),
            "hoist" => {
                config.hoist = defaults.hoist;
                reset_hoist_pattern(config, defaults);
            }
            "hoistPattern" => reset_hoist_pattern(config, defaults),
            "shamefullyHoist" => {
                config.shamefully_hoist = defaults.shamefully_hoist;
                reset_public_hoist_pattern(config, defaults);
            }
            "publicHoistPattern" => reset_public_hoist_pattern(config, defaults),
            "nodeLinker" => {
                config.node_linker = defaults.node_linker;
                config.apply_prefer_symlinked_executables_derivation();
            }
            "preferSymlinkedExecutables" => {
                config.prefer_symlinked_executables = None;
                config.apply_prefer_symlinked_executables_derivation();
            }
            "virtualStoreOnly" => {
                config.virtual_store_only = defaults.virtual_store_only;
                leave_virtual_store_only(config);
            }
            _ => return Self::reset_aliased_setting_to_default(config, defaults, key),
        }
        true
    }

    pub(super) fn reset_aliased_setting_to_default(
        config: &mut Config,
        defaults: &Config,
        key: &str,
    ) -> bool {
        match key {
            "packages" => {
                config.workspace_package_patterns.clone_from(&defaults.workspace_package_patterns);
            }
            "gitBranchLockfile" => {
                config.use_git_branch_lockfile = defaults.use_git_branch_lockfile;
                config.git_branch_lockfile_name = None;
            }
            "sideEffectsCache" => {
                config.side_effects_cache_read_setting = defaults.side_effects_cache_read_setting;
                config.side_effects_cache_write_setting = defaults.side_effects_cache_write_setting;
                config.remote_side_effects_cache.clone_from(&defaults.remote_side_effects_cache);
            }
            "httpsProxy" | "httpProxy" | "proxy" | "noProxy" | "noproxy" => {
                reset_proxy_setting(config, defaults, key);
            }
            "audit" | "auditLevel" | "auditConfig" => {
                config.audit_level = defaults.audit_level;
                config.audit_config.clone_from(&defaults.audit_config);
                config.audit_ignore_prune = defaults.audit_ignore_prune;
            }
            "update" | "updateConfig" => config.update_config.clone_from(&defaults.update_config),
            "cleanupUnusedCatalogs" => config.catalog_prune = defaults.catalog_prune,
            "virtualStoreType" => {
                config.enable_global_virtual_store = defaults.enable_global_virtual_store;
            }
            "maxsockets" => config.max_sockets = defaults.max_sockets,
            // Shapes only a file has, whose resolved form lives under the
            // keys above, or one nothing resolves from: nothing to restore.
            "registries"
            | "namedRegistries"
            | "catalog"
            | "onlyBuiltDependencies"
            | "neverBuiltDependencies"
            | "ignoredBuiltDependencies"
            | "_auth" => {}
            _ => return false,
        }
        true
    }
}
