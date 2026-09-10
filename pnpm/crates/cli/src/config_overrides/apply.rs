use super::{
    Config, ConfigOverrides, EnvVar, GLOBAL_LAYOUT_VERSION, GetCurrentDir, GetHomeDir, LinkProbe,
    Path, StoreDir, default_state_dir, lexical_normalize, resolve_child_concurrency, setting_value,
    verify_deps_env_is_set,
};

pub(crate) fn apply_store_dir_override<Sys>(
    config: &mut Config,
    store_dir: &Path,
    dir: &Path,
) -> miette::Result<()>
where
    Sys: EnvVar + GetCurrentDir + GetHomeDir + LinkProbe,
{
    let workspace_dir = config.workspace_dir.as_deref().unwrap_or(dir).to_path_buf();
    if store_dir.as_os_str().is_empty() {
        config.reset_store_dir_to_default::<Sys>(&workspace_dir);
        config
            .explicit_settings
            .insert("storeDir".to_string(), serde_json::Value::String(String::new()));
        return Ok(());
    }
    let resolved = if let Some(relative) = home_relative_store_dir(store_dir) {
        Sys::home_dir()
            .ok_or_else(|| {
                let store_dir_display = store_dir.display();
                miette::miette!(
                    "Cannot resolve store directory {} because the home directory is unknown",
                    store_dir_display,
                )
            })?
            .join(relative)
    } else if store_dir.is_absolute() {
        store_dir.to_path_buf()
    } else {
        workspace_dir.join(store_dir)
    };
    config.store_dir = StoreDir::from(lexical_normalize(&resolved));
    if let Some(store_dir) = store_dir.to_str() {
        config
            .explicit_settings
            .insert("storeDir".to_string(), serde_json::Value::String(store_dir.to_string()));
    }
    let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
    let global_virtual_store_dir_explicit =
        config.explicit_settings.contains_key("globalVirtualStoreDir");
    config.apply_global_virtual_store_derivation(
        virtual_store_dir_explicit,
        global_virtual_store_dir_explicit,
    );
    Ok(())
}

pub(crate) fn apply_state_dir_override<Sys>(config: &mut Config, state_dir: &Path, dir: &Path)
where
    Sys: EnvVar + GetHomeDir,
{
    config.state_dir = if state_dir.as_os_str().is_empty() {
        default_state_dir::<Sys>().unwrap_or_default()
    } else if state_dir.is_absolute() {
        lexical_normalize(state_dir)
    } else {
        lexical_normalize(&dir.join(state_dir))
    };
    if let Some(state_dir) = state_dir.to_str() {
        config
            .explicit_settings
            .insert("stateDir".to_string(), serde_json::Value::String(state_dir.to_string()));
    }
}

fn home_relative_store_dir(store_dir: &Path) -> Option<&Path> {
    let store_dir = store_dir.to_str()?;
    store_dir.strip_prefix("~/").or_else(|| store_dir.strip_prefix(r"~\")).map(Path::new)
}

/// Layer a registry URL (the universal `--registry` flag or a
/// `--config.registry=<url>` override) onto `config`, setting the default
/// registry everywhere it is read: the resolved registry, the `default`
/// entry of the named-registry map, and the package-manager bootstrap
/// copies of both. The URL is normalized to a trailing slash first, so an
/// already-normalized override applies idempotently.
pub(crate) fn apply_registry_override(config: &mut Config, registry: &str) {
    let registry = normalize_registry_url(registry);
    config.registry.clone_from(&registry);
    config.registries_by_scope.insert("default".to_string(), registry.clone());
    config.package_manager_bootstrap.registry.clone_from(&registry);
    config.package_manager_bootstrap.registries.insert("default".to_string(), registry.clone());
    config.explicit_settings.insert("registry".to_string(), serde_json::Value::String(registry));
}

pub(super) fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

impl ConfigOverrides {
    /// Layer the CLI overrides on top of a [`Config`] that has already
    /// been built from defaults, `.npmrc`, and `pnpm-workspace.yaml`.
    /// Mirrors pnpm 11's "CLI > yaml > .npmrc > defaults" precedence.
    ///
    /// `dir` is the canonicalized `--dir`, the fallback base for a
    /// relative path-valued setting outside a workspace.
    pub fn apply(&self, config: &mut Config, dir: &Path) {
        config.apply_proxy_cli_overrides(
            self.https_proxy.as_deref(),
            self.http_proxy.as_deref(),
            self.no_proxy.as_deref(),
        );
        record_overrides!(self, config, allow_unused_patches => "allowUnusedPatches");
        copy_overrides!(
            self,
            config,
            bail,
            ci,
            color,
            embed_readme,
            ignore_workspace_root_check,
            optional,
        );
        self.apply_lockfile_overrides(config);
        copy_overrides!(self, config, pending, recursive_install, reverse);
        self.apply_hoist_overrides(config);
        copy_overrides!(
            self,
            config,
            shell_emulator,
            skip_manifest_obfuscation,
            sort,
            use_beta_cli
        );
        self.apply_registry_overrides(config);
        copy_overrides!(self, config, deploy_all_files, force_legacy_deploy);
        // `pnpm config get ignore-scripts` answers from the explicitly-set
        // settings, so a CLI-set value has to be recorded there to be
        // reported as set while it suppresses the scripts.
        record_overrides!(self, config, ignore_scripts => "ignoreScripts");
        copy_overrides!(self, config, inject_workspace_packages);
        self.apply_socket_and_release_age_overrides(config);
        self.apply_linker_and_run_overrides(config);
        self.apply_trust_and_cache_overrides(config);
        self.apply_install_policy_overrides(config);
        self.apply_global_directory(config, dir);
        self.apply_lockfile_anchored_paths(config, dir);
    }

    fn apply_global_directory(&self, config: &mut Config, dir: &Path) {
        if let Some(value) = self.global_dir.as_deref().filter(|value| !value.is_empty()) {
            let global_dir = lexical_normalize(&dir.join(value));
            config.global_pkg_dir = Some(global_dir.join(GLOBAL_LAYOUT_VERSION));
            config.global_dir = Some(global_dir);
            config.explicit_settings.insert("globalDir".to_string(), value.into());
        }
    }

    fn apply_trust_and_cache_overrides(&self, config: &mut Config) {
        record_overrides!(
            self,
            config,
            side_effects_cache_readonly => "sideEffectsCacheReadonly",
            optimistic_repeat_install => "optimisticRepeatInstall",
            trust_lockfile => "trustLockfile",
        );
        record_enum_overrides!(self, config, trust_policy => "trustPolicy");
        record_list_overrides!(self, config, trust_policy_exclude => "trustPolicyExclude");
        if let Some(value) = self.trust_policy_ignore_after {
            config.trust_policy_ignore_after = Some(value);
            config.explicit_settings.insert("trustPolicyIgnoreAfter".to_string(), value.into());
        }
    }

    fn apply_install_policy_overrides(&self, config: &mut Config) {
        record_overrides!(
            self,
            config,
            unsafe_perm => "unsafePerm",
            dangerously_allow_all_builds => "dangerouslyAllowAllBuilds",
            engine_strict => "engineStrict",
            frozen_store => "frozenStore",
            ignore_pnpmfile => "ignorePnpmfile",
        );
        record_enum_overrides!(self, config, link_workspace_packages => "linkWorkspacePackages");
        record_overrides!(
            self,
            config,
            lockfile_include_tarball_url => "lockfileIncludeTarballUrl",
            merge_git_branch_lockfiles => "mergeGitBranchLockfiles",
            node_experimental_package_map => "nodeExperimentalPackageMap",
            offline => "offline",
            prefer_frozen_lockfile => "preferFrozenLockfile",
            prefer_offline => "preferOffline",
        );
        record_enum_overrides!(self, config, save_workspace_protocol => "saveWorkspaceProtocol");
        record_overrides!(self, config, verify_store_integrity => "verifyStoreIntegrity");
    }

    /// `--no-package-lock` is npm's spelling of `--no-lockfile`, so it
    /// stands in for the setting only when nothing else has set it.
    fn apply_lockfile_overrides(&self, config: &mut Config) {
        if let Some(value) = self.package_lock {
            config.package_lock = value;
            if self.lockfile.is_none() && !config.explicit_settings.contains_key("lockfile") {
                config.lockfile = value;
            }
        }
        record_overrides!(self, config, lockfile => "lockfile");
    }

    /// `virtualStoreOnly` comes first: a `--no-virtual-store-only` gets the
    /// lower layers' patterns back before a pattern on the same command
    /// line replaces them.
    fn apply_hoist_overrides(&self, config: &mut Config) {
        if let Some(value) = self.virtual_store_only {
            config.virtual_store_only = value;
            config.explicit_settings.insert("virtualStoreOnly".to_string(), value.into());
            if value {
                config.apply_virtual_store_only_derivation();
            } else {
                config.restore_hoist_patterns_after_virtual_store_only();
            }
        }
        record_overrides!(
            self,
            config,
            shamefully_hoist => "shamefullyHoist",
            hoist => "hoist",
        );
        record_list_overrides!(
            self,
            config,
            hoist_pattern => "hoistPattern",
            public_hoist_pattern => "publicHoistPattern",
        );
        if self.shamefully_hoist.is_some()
            || self.hoist.is_some()
            || self.hoist_pattern.is_some()
            || self.public_hoist_pattern.is_some()
        {
            // `hoist: false` nullifies the private pattern whichever layer
            // supplied either, so the two derivations below re-run over the
            // command line's contribution the way `WorkspaceSettings::apply_to`
            // runs them over yaml's.
            if !config.hoist {
                config.hoist_pattern = None;
            }
            config.apply_shamefully_hoist_derivation();
            config.apply_virtual_store_only_derivation();
        }
    }

    fn apply_registry_overrides(&self, config: &mut Config) {
        if let Some(registry) = &self.registry {
            apply_registry_override(config, registry);
        }
        if let Some(scope) = &self.scope {
            config.scope = Some(scope.clone());
        }
        for (scope, registry) in &self.registries {
            config.registries_by_scope.insert(scope.clone(), registry.clone());
            config.package_manager_bootstrap.registries.insert(scope.clone(), registry.clone());
        }
    }

    fn apply_socket_and_release_age_overrides(&self, config: &mut Config) {
        // npm's spelling first, so the canonical one wins when a single
        // command line carries both.
        if let Some(value) = self.maxsockets {
            config.max_sockets = Some(value);
        }
        if let Some(value) = self.max_sockets {
            config.max_sockets = Some(value);
        }
        // pnpm seeds `explicitlySetKeys` from the command line as well as
        // from the config files, and the workspace state reads it back to
        // decide whether `minimumReleaseAgeStrict` defaults to true.
        if let Some(value) = self.minimum_release_age {
            config.minimum_release_age = Some(value);
            config.explicit_settings.insert("minimumReleaseAge".to_string(), value.into());
        }
        record_list_overrides!(
            self,
            config,
            minimum_release_age_exclude => "minimumReleaseAgeExclude",
        );
        record_overrides!(
            self,
            config,
            minimum_release_age_ignore_missing_time => "minimumReleaseAgeIgnoreMissingTime",
        );
        if let Some(value) = self.minimum_release_age_strict {
            config.minimum_release_age_strict = Some(value);
            config.explicit_settings.insert("minimumReleaseAgeStrict".to_string(), value.into());
        }
    }

    fn apply_linker_and_run_overrides(&self, config: &mut Config) {
        if let Some(value) = self.node_linker {
            config.node_linker = value;
            // A CLI-selected hoisted linker turns the default on just
            // like a yaml-selected one — pnpm merges CLI options before
            // its `nodeLinker` switch, so the derivation must see this
            // override too.
            config.apply_prefer_symlinked_executables_derivation();
        }
        if let Some(value) = self.pm_on_fail {
            config.pm_on_fail = Some(value);
        }
        if let Some(value) = self.runtime_on_fail {
            config.runtime_on_fail = Some(value);
        }
        record_overrides!(self, config, shared_workspace_lockfile => "sharedWorkspaceLockfile");
        // The `pnpm_config_verify_deps_before_run` env var outranks even
        // the CLI for this one key (pnpm's config reader applies it after
        // every other layer): pnpm stamps `false` into every spawned
        // script's env, and a nested `pnpm run` inside a script must see
        // the check disabled no matter what flags the outer invocation
        // carried, or the spawned install's lifecycle scripts would
        // re-enter the check (pnpm/pnpm#10060).
        if let Some(value) = self.verify_deps_before_run
            && !verify_deps_env_is_set()
        {
            config.verify_deps_before_run = value;
        }
        record_enum_overrides!(self, config, package_import_method => "packageImportMethod");
        if let Some(value) = self.child_concurrency {
            config.child_concurrency = resolve_child_concurrency(Some(value));
            config.explicit_settings.insert("childConcurrency".to_string(), value.into());
        }
        record_overrides!(self, config, strict_peer_dependencies => "strictPeerDependencies");
        if let Some(value) = self.side_effects_cache {
            config.apply_side_effects_cache_shorthand(value);
            config.explicit_settings.insert("sideEffectsCache".to_string(), value.into());
        }
    }

    /// Re-anchor the root `node_modules` and the virtual store onto a
    /// command-line `--modules-dir` / `--virtual-store-dir`.
    ///
    /// Both reach [`Config::explicit_settings`] as the raw spelling, which
    /// is what [`Config::anchor_lockfile_paths`] resolves — so a later
    /// `--lockfile-dir` pin moves the CLI-set paths along with it.
    fn apply_lockfile_anchored_paths(&self, config: &mut Config, dir: &Path) {
        let raw_settings = [
            ("modulesDir", self.modules_dir.as_deref()),
            ("virtualStoreDir", self.virtual_store_dir.as_deref()),
        ];
        let mut anchored = false;
        for (setting, value) in raw_settings {
            if let Some(value) = value {
                config.explicit_settings.insert(setting.to_string(), value.into());
                anchored = true;
            }
        }
        if !anchored {
            return;
        }
        let anchor = config
            .lockfile_dir
            .clone()
            .or_else(|| config.workspace_dir.clone())
            .unwrap_or_else(|| dir.to_path_buf());
        config.anchor_lockfile_paths(&anchor);
        let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            config.explicit_settings.contains_key("globalVirtualStoreDir");
        config.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }
}
