use super::{
    AllowBuild, Config, PackageConfigsSetting, PnpmfileSetting, WorkspaceSettings, as_set,
    global_shims_setting, opt_path, path, side_effects_cache_setting,
};

impl WorkspaceSettings {
    /// Every setting at the value `config` resolved it to, for a consumer
    /// that must read the effective configuration rather than one file's
    /// contribution to it.
    ///
    /// The inverse of [`Self::apply_to`]: applying the result to a default
    /// [`Config`] reproduces `config`'s settings. Defaults are therefore
    /// present as values, not as `None` — `None` here means "pnpm has no
    /// such setting to report", which is true of exactly two groups:
    ///
    /// - Keys that name a shape only a file has, whose resolved form
    ///   [`Config`] keeps under a different name. `registries` becomes the
    ///   `registriesByScope` / `registriesByPrefix` lookups, `catalog`
    ///   becomes the `default` entry of [`Self::catalogs`], and the build
    ///   allow-lists become one `allowBuilds` record.
    /// - Deprecated spellings, which report under the canonical one:
    ///   `maxsockets`, `noproxy`, `updateConfig`, `auditLevel`,
    ///   `auditConfig`, `cleanupUnusedCatalogs`, `namedRegistries`, and
    ///   `virtualStoreType`.
    ///
    /// `_auth` stays `None` because a project file may not carry it, and
    /// [`Self::key_issues`] is diagnostics about one file rather than a
    /// setting.
    ///
    /// Two groups report as the user set them rather than as they resolved,
    /// and so read as `None` when nothing set them: the path settings
    /// [`Self::apply_to`] anchors against a base directory, and the settings
    /// pnpm reads for whether they were set at all. See the comments at their
    /// assignments for why each must. `cacheDir` belongs to the first group
    /// but falls back to the resolved directory, as pnpm reports it.
    #[must_use]
    pub fn from_resolved(config: &Config) -> Self {
        macro_rules! read {
            ($($field:ident),* $(,)?) => {
                Self { $($field: Some(config.$field.clone()),)* ..Self::default() }
            };
        }
        let mut settings = identically_named_settings!(read);

        settings = settings.with_resolved_paths(config);
        settings = settings.with_resolved_presence(config);
        settings = settings.with_resolved_scripts(config);
        settings = settings.with_resolved_policy(config);
        settings = settings.with_resolved_collections(config);
        settings
    }

    pub(super) fn with_resolved_paths(self, config: &Config) -> Self {
        Self {
            hoist_pattern: Some(config.hoist_pattern.clone()),
            public_hoist_pattern: Some(config.public_hoist_pattern.clone()),
            state_dir: Some(path(&config.state_dir)),
            lockfile_dir: opt_path(config.lockfile_dir.as_deref()),
            npmrc_auth_file: opt_path(config.npmrc_auth_file.as_deref()),
            global_pnpmfile: opt_path(config.global_pnpmfile.as_deref()),
            pnpmfile: config
                .pnpmfile
                .as_ref()
                .map(|paths| PnpmfileSetting::Multiple(paths.iter().map(|p| path(p)).collect())),

            // The path settings `apply_to` anchors against the file's
            // directory report as the user wrote them, relative form
            // included, matching pnpm. Anchoring happens when a value is
            // applied, so reporting the anchored path would re-anchor a
            // relative setting against wherever the hook's answer is
            // applied from.
            store_dir: as_set(config, "storeDir"),
            modules_dir: as_set(config, "modulesDir"),
            virtual_store_dir: as_set(config, "virtualStoreDir"),
            global_virtual_store_dir: as_set(config, "globalVirtualStoreDir"),
            global_dir: as_set(config, "globalDir"),
            global_bin_dir: as_set(config, "globalBinDir"),

            // `cacheDir` is one of those settings when a source set it, and
            // the directory pnpm chose for the host when none did, which is
            // where the cache is read and written either way.
            cache_dir: as_set(config, "cacheDir").or_else(|| Some(path(&config.cache_dir))),
            ..self
        }
    }

    pub(super) fn with_resolved_presence(self, config: &Config) -> Self {
        Self {
            // A setting pnpm reads for *whether* it was set, not only for
            // its value, reports as the user set it. Reporting the resolved
            // value instead would leave a hook assigning that same value
            // with no change for the caller to notice, and the setting
            // would go on counting as unset.
            prefer_frozen_lockfile: as_set(config, "preferFrozenLockfile"),
            lockfile: as_set(config, "lockfile"),
            shamefully_hoist: as_set(config, "shamefullyHoist"),
            merge_git_branch_lockfiles: as_set(config, "mergeGitBranchLockfiles"),
            optimistic_repeat_install: as_set(config, "optimisticRepeatInstall"),
            minimum_release_age: as_set(config, "minimumReleaseAge"),
            prefer_symlinked_executables: as_set(config, "preferSymlinkedExecutables"),

            global_shims: Some(global_shims_setting(config)),

            frozen_lockfile: config.frozen_lockfile,
            git_branch_lockfile: Some(config.use_git_branch_lockfile),
            registry: Some(config.registry.clone()),
            scope: config.scope.clone(),
            pnpr_server: config.pnpr_server.clone(),
            cargo: Some(config.cargo.clone()),
            python: Some(config.python.clone()),
            remote_side_effects_cache: config.remote_side_effects_cache.clone(),
            reporter_hide_prefix: config.reporter_hide_prefix,
            max_sockets: config.max_sockets,
            patched_dependencies: config.patched_dependencies.clone(),
            patches_dir: config.patches_dir.clone(),
            config_dependencies: config.config_dependencies.clone(),
            packages: config.workspace_package_patterns.clone(),
            ..self
        }
    }

    pub(super) fn with_resolved_scripts(self, config: &Config) -> Self {
        Self {
            dangerously_allow_all_builds: Some(config.dangerously_allow_all_builds),
            strict_dep_builds: Some(config.strict_dep_builds),
            ignore_scripts: Some(config.ignore_scripts),
            ignore_pnpmfile: Some(config.ignore_pnpmfile),
            git_checks: Some(config.git_checks),
            engine_strict: Some(config.engine_strict),
            node_version: config.node_version.clone(),
            runtime_on_fail: config.runtime_on_fail,
            node_download_mirrors: Some(config.node_download_mirrors.clone()),
            scripts_prepend_node_path: Some(config.scripts_prepend_node_path),
            script_shell: Some(config.script_shell.clone()),
            node_options: Some(config.node_options.clone()),
            unsafe_perm: Some(config.unsafe_perm),
            supported_architectures: config.supported_architectures.clone(),
            ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
            overrides: config.overrides.clone(),
            package_extensions: config.package_extensions.clone(),
            // The flattened lookup, which is the by-name form of the setting
            // whichever of the two forms the file wrote it in.
            package_configs: config.package_configs.clone().map(PackageConfigsSetting::ByName),
            ..self
        }
    }

    pub(super) fn with_resolved_policy(self, config: &Config) -> Self {
        Self {
            minimum_release_age_exclude: config.minimum_release_age_exclude.clone(),
            minimum_release_age_ignore_missing_time: Some(
                config.minimum_release_age_ignore_missing_time,
            ),
            minimum_release_age_strict: config.minimum_release_age_strict,
            trust_lockfile: Some(config.trust_lockfile),
            trust_policy: Some(config.trust_policy),
            trust_policy_exclude: config.trust_policy_exclude.clone(),
            trust_policy_exclude_prune: Some(config.trust_policy_exclude_prune),
            trust_policy_ignore_after: config.trust_policy_ignore_after,
            init_author_name: config.init_author_name.clone(),
            init_author_email: config.init_author_email.clone(),
            init_author_url: config.init_author_url.clone(),
            init_license: config.init_license.clone(),
            init_version: config.init_version.clone(),
            pm_on_fail: config.pm_on_fail,
            versioning: Some(config.versioning.clone()),
            save_catalog_name: config.save_catalog_name.clone(),
            save_prefix: config.save_prefix.clone(),
            pipeline_base: config.pipeline_base.clone(),

            // `child_concurrency` / `workspace_concurrency` are resolved to
            // a positive count on `Config`, which the settings hold as the
            // signed type the file's negative "all but N cores" forms need.
            child_concurrency: Some(i32::try_from(config.child_concurrency).unwrap_or(i32::MAX)),
            workspace_concurrency: Some(
                i32::try_from(config.workspace_concurrency).unwrap_or(i32::MAX),
            ),

            side_effects_cache: Some(side_effects_cache_setting(config)),
            ..self
        }
    }

    pub(super) fn with_resolved_collections(self, config: &Config) -> Self {
        Self {
            catalogs: config.catalogs.as_ref().map(|catalogs| {
                catalogs
                    .iter()
                    .map(|(name, entries)| {
                        (
                            name.clone(),
                            entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                        )
                    })
                    .collect()
            }),
            allow_builds: Some(
                config
                    .allow_builds
                    .iter()
                    .map(|(name, allowed)| (name.clone(), AllowBuild::Decided(*allowed)))
                    .collect(),
            ),

            https_proxy: config.proxy.https_proxy.clone(),
            http_proxy: config.proxy.http_proxy.clone(),
            no_proxy: config.proxy.no_proxy.as_ref().map(|no_proxy| match no_proxy {
                pnpm_network::NoProxySetting::Bypass => serde_json::Value::Bool(true),
                pnpm_network::NoProxySetting::List(hosts) => {
                    serde_json::Value::String(hosts.join(","))
                }
            }),

            // `audit` and `update` are the canonical spellings, so the
            // deprecated `auditLevel`, `auditConfig`, and `updateConfig`
            // stay unset rather than restating them.
            audit: config.resolved_audit_settings(),
            update: config.resolved_update_settings(),
            update_config: None,
            ..self
        }
    }
}
