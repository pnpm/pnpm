use super::{
    AuthSources, Config, EnvVar, EnvVarOs, ExplicitPaths, GetCurrentDir, GetHomeDir, LinkProbe,
    LoadWorkspaceYamlError, NpmrcAuth, Path, WorkspaceSettings, build_package_manager_bootstrap,
    collect_explicit_settings, default_config_dir, default_state_dir, resolve_configured_state_dir,
};

impl Config {
    /// Load the merged configuration for a CLI run.
    ///
    /// Config sources (low → high precedence): `SmartDefault`, the supported
    /// `.npmrc` subset (cwd, falling back to home), global `config.yaml`,
    /// project `pnpm-workspace.yaml`, then `PNPM_CONFIG_*` env.
    ///
    /// Pacquet currently applies `registry`, scoped registry routes,
    /// npm-auth credentials, the
    /// proxy keys (`https-proxy`, `http-proxy`, `proxy`, `no-proxy` /
    /// `noproxy`), and the TLS + local-address keys (`ca`, `cafile`,
    /// `cert`, `key`, `strict-ssl`, `local-address`) from `.npmrc`.
    /// Other `.npmrc` entries — project-structural settings like
    /// `storeDir`, `lockfile` and `hoist-pattern` — are silently
    /// ignored here. Those must come from `pnpm-workspace.yaml` or CLI
    /// flags, matching pnpm 11.
    ///
    /// Returns [`LoadWorkspaceYamlError`] when an existing
    /// `pnpm-workspace.yaml` cannot be read or parsed. A missing file is not
    /// an error.
    pub fn current<Sys>(self, start_dir: &std::path::Path) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        self.current_inner::<Sys>(start_dir, false)
    }

    /// Like [`Config::current`], but the project `pnpm-workspace.yaml` does
    /// not contribute the `minimumReleaseAge` / `trustPolicy` policies — see
    /// [`WorkspaceSettings::clear_self_update_policy`].
    pub fn current_for_self_update<Sys>(
        self,
        start_dir: &std::path::Path,
    ) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        self.current_inner::<Sys>(start_dir, true)
    }

    pub(super) fn current_inner<Sys>(
        mut self,
        start_dir: &std::path::Path,
        for_self_update: bool,
    ) -> Result<Self, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        let default_state_dir = default_state_dir::<Sys>().unwrap_or_default();
        self.state_dir.clone_from(&default_state_dir);

        self.anchor_default_module_dirs(start_dir);

        // Read the project/workspace .npmrc plus trusted user-level sources
        // and apply only the auth/network subset. Everything else is
        // intentionally ignored.
        //
        // pnpm reads several `.npmrc` sources and merges them
        // (`user < auth.ini < workspace`), pinning each file's *unscoped*
        // credentials to that file's own registry *before* the merge so
        // a higher-priority file (or `pnpm-workspace.yaml`) can never
        // pull them to a different host. See
        // [`NpmrcAuth::rescope_unscoped`].
        //
        // The global `config.yaml` is loaded up front: its `npmrcAuthFile`
        // participates in the user-level path resolution below, and its
        // directory is where `auth.ini` lives.
        let global_config_dir = default_config_dir::<Sys>();
        self.config_dir.clone_from(&global_config_dir);
        let global_settings = self.load_global_settings::<Sys>()?;

        let workspace_yaml = self.resolve_workspace_yaml::<Sys>(start_dir)?;

        let AuthSources { mut npmrc_auth, trusted_auth } = self.collect_auth_sources::<Sys>(
            start_dir,
            workspace_yaml.as_ref(),
            global_settings.as_ref(),
            global_config_dir.as_deref(),
        )?;

        self.apply_bootstrap_settings::<Sys>(trusted_auth, global_settings.as_ref())?;

        // Collected as each file is applied, since applying it is what makes
        // a declared route indistinguishable by value from a resolved one.
        let mut declared_registries = crate::npmrc_auth::DeclaredRegistries::default();
        self.apply_npmrc_settings::<Sys>(&mut npmrc_auth, &mut declared_registries);

        // The "did the user pin this path?" signals, threaded through every
        // layer so the derivations below can tell a pinned value from a
        // `SmartDefault` fallback.
        let mut explicit = ExplicitPaths::default();
        self.apply_global_settings::<Sys>(
            global_settings,
            &mut explicit,
            &mut declared_registries,
            &default_state_dir,
            start_dir,
        );

        self.apply_workspace_yaml::<Sys>(
            workspace_yaml,
            &mut explicit,
            &mut declared_registries,
            for_self_update,
        )?;

        // Apply `_auth` routes after workspace yaml (so they win over
        // repo-controlled registries) but before `PNPM_CONFIG_*` (so an
        // explicit `pnpm_config_registry` / `--registry` still wins) —
        // pnpm's "CLI > _auth > yaml" precedence.
        npmrc_auth.apply_json_env_registries(&mut self, &declared_registries);

        self.apply_env_settings::<Sys>(&mut explicit, &default_state_dir, start_dir);

        if !self.explicit_settings.contains_key("lockfile") {
            self.lockfile = self.package_lock;
        }

        self.apply_store_derivations::<Sys>(explicit, &mut npmrc_auth, start_dir)?;

        self.apply_layout_derivations::<Sys>();

        Ok(self)
    }

    /// Anchor module defaults to the requested directory, which may differ from the process cwd.
    pub(super) fn anchor_default_module_dirs(&mut self, start_dir: &std::path::Path) {
        self.modules_dir = start_dir.join("node_modules");
        self.virtual_store_dir = self.modules_dir.join(".pnpm");
    }

    pub(super) fn apply_bootstrap_settings<Sys: EnvVar>(
        &mut self,
        trusted_auth: NpmrcAuth,
        global_settings: Option<&WorkspaceSettings>,
    ) -> Result<(), LoadWorkspaceYamlError> {
        self.package_manager_bootstrap = build_package_manager_bootstrap::<Sys>(trusted_auth)?;
        if let Some(global_settings) = global_settings {
            let bootstrap = &mut self.package_manager_bootstrap;
            global_settings.apply_proxy_to(&mut bootstrap.proxy, &mut bootstrap.proxy_keys);
        }
        Ok(())
    }

    pub(super) fn apply_npmrc_settings<Sys: EnvVar>(
        &mut self,
        npmrc_auth: &mut NpmrcAuth,
        declared_registries: &mut crate::npmrc_auth::DeclaredRegistries,
    ) {
        npmrc_auth.apply_registry_and_warn(self, declared_registries);
        // Proxy cascade fires unconditionally — even when no `.npmrc`
        // is found — because the env-var fallback is a normalization step
        // on the resolved config, not a function of `.npmrc` presence.
        npmrc_auth.apply_proxy_cascade::<Sys>(self);
        // TLS + local-address are sourced from `.npmrc` only — pnpm
        // does not honor env vars (`NODE_EXTRA_CA_CERTS`,
        // `NODE_TLS_REJECT_UNAUTHORIZED`, etc.) for these keys
        // (Node's runtime does, but pnpm's reader does not). When
        // there is no `.npmrc`, `npmrc_auth` is the default value and
        // this is a no-op write of `TlsConfig::default()` onto the
        // already-default `self.tls`.
        npmrc_auth.apply_tls_and_local_address(self);
    }

    pub(super) fn load_global_settings<Sys: EnvVar>(
        &self,
    ) -> Result<Option<WorkspaceSettings>, LoadWorkspaceYamlError> {
        let mut global_settings =
            self.config_dir.as_deref().map(WorkspaceSettings::load_global).transpose()?.flatten();
        if let Some(global_settings) = global_settings.as_mut() {
            global_settings.substitute_env_trusted::<Sys>();
        }

        Ok(global_settings)
    }

    /// Apply `PNPM_CONFIG_*` env vars *after* `pnpm-workspace.yaml`:
    /// env vars override yaml. The `WorkspaceSettings::apply_to`
    /// call also runs the post-processing (Windows `unsafe_perm`
    /// override, `hoist: false` short-circuit on `hoist_pattern`)
    /// regardless of where the values came from, so env-var-set
    /// values still go through the same hardening yaml-set values
    /// do.
    ///
    /// `workspace_dir` save/restore is the same trick used for the
    /// global config above — `apply_to` would otherwise clobber
    /// `workspace_dir` with `start_dir`, hiding the workspace yaml's
    /// location (or, if there was no yaml, setting it to a value
    /// that doesn't actually correspond to a discovered workspace).
    pub(super) fn apply_env_settings<Sys>(
        &mut self,
        explicit: &mut ExplicitPaths,
        default_state_dir: &Path,
        start_dir: &Path,
    ) where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        let mut env_settings = WorkspaceSettings::from_pnpm_config_env::<Sys>();
        explicit.note(&env_settings);
        env_settings.substitute_env_trusted::<Sys>();
        // `PNPM_CONFIG_REGISTRY` comes from the environment, not the
        // repository, so it overrides the bootstrap default registry too.
        let env_registry_override = env_settings.registry.clone();
        collect_explicit_settings(&mut self.explicit_settings, &env_settings);
        let configured_state_dir = env_settings.state_dir.take();
        let bootstrap = &mut self.package_manager_bootstrap;
        env_settings.apply_proxy_to(&mut bootstrap.proxy, &mut bootstrap.proxy_keys);
        let saved_workspace_dir = self.workspace_dir.clone();
        env_settings.expand_global_dir_home_prefixes::<Sys>();
        env_settings.apply_to(self, start_dir);
        self.workspace_dir = saved_workspace_dir;
        self.apply_remote_side_effects_cache_env::<Sys>();
        if let Some(configured_state_dir) =
            configured_state_dir.as_deref().filter(|value| !value.is_empty())
        {
            self.state_dir = resolve_configured_state_dir(default_state_dir, configured_state_dir);
        }
        if let Some(registry) = env_registry_override {
            let normalized =
                if registry.ends_with('/') { registry } else { format!("{registry}/") };
            self.registries_by_scope.insert("default".to_string(), normalized.clone());
            self.package_manager_bootstrap.registry.clone_from(&normalized);
            self.package_manager_bootstrap.registries.insert("default".to_string(), normalized);
        }
    }
}
