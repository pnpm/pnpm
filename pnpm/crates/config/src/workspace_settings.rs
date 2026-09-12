use super::{
    Config, EnvVar, EnvVarOs, ExplicitPaths, GetCurrentDir, GetHomeDir, LinkProbe,
    LoadWorkspaceYamlError, Path, PathBuf, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings,
    collect_explicit_settings, fs, note_declared_registries, resolve_configured_state_dir,
};

impl Config {
    /// Apply the workspace layer and anchor paths to its location. A missing file
    /// is silent; read and parse errors propagate during workspace discovery.
    pub(super) fn apply_workspace_yaml<Sys>(
        &mut self,
        workspace_yaml: Option<(PathBuf, Option<WorkspaceSettings>)>,
        explicit: &mut ExplicitPaths,
        declared_registries: &mut crate::npmrc_auth::DeclaredRegistries,
        for_self_update: bool,
    ) -> Result<(), LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        // Capture the "did yaml set this field" booleans *before*
        // applying yaml so the GVS derivation downstream can tell apart
        // user-pinned values from SmartDefault fallbacks. Without these
        // signals the derivation would always see populated values
        // (SmartDefault wrote them in) and would either always or never
        // re-point them, neither of which is correct.
        if let Some((base_dir, settings)) = workspace_yaml {
            // Re-anchor the path-valued defaults to the workspace root
            // before applying settings. Without this, a `pacquet install`
            // run from a workspace subdirectory leaves
            // `modules_dir` / `virtual_store_dir` anchored at the CLI
            // `--dir` (the subdir), while the per-importer
            // [`SymlinkDirectDependencies`] writes are anchored at the
            // workspace root — producing two `node_modules` layouts
            // for the same install. pnpm v11 ties
            // `pnpmConfig.dir = lockfileDir` exactly so its defaults
            // resolve from the workspace root; we mirror that here.
            //
            // Applied *before* `settings.apply_to` so an explicit
            // `modulesDir` / `virtualStoreDir` in `pnpm-workspace.yaml`
            // still wins.
            //
            // `virtual_store_dir_explicit` guards the re-anchor for
            // `virtual_store_dir` — without it, a `virtualStoreDir`
            // already set in the global `config.yaml` would be
            // clobbered by the workspace-root default whenever the
            // workspace yaml itself leaves the field unset. `modules_dir`
            // needs no such guard because pnpm's `excludedPnpmKeys`
            // (and pacquet's `clear_workspace_only_fields`) keep it
            // out of the global-config surface, so it can only come
            // from workspace yaml or env vars, and env vars haven't
            // been applied yet at this point in the cascade.
            self.modules_dir = base_dir.join("node_modules");
            if !explicit.virtual_store_dir {
                self.virtual_store_dir = base_dir.join("node_modules").join(".pnpm");
            }
            // The workspace root is structural context (env-lockfile reads/
            // writes, pin persistence), not a "setting" — set it whenever a
            // workspace is discovered, even on the `NPM_CONFIG_WORKSPACE_DIR`
            // path when the yaml file is missing and `apply_to` (which also
            // writes it) never runs.
            self.workspace_dir = Some(base_dir.clone());
            self.workspace_package_patterns = Some(
                settings
                    .as_ref()
                    .and_then(|settings| settings.packages.clone())
                    .unwrap_or_else(|| vec![".".to_string()]),
            );
            if let Some(settings) = settings {
                self.apply_workspace_settings::<Sys>(
                    settings,
                    &base_dir,
                    explicit,
                    declared_registries,
                    for_self_update,
                )?;
            }
        }
        Ok(())
    }

    /// The workspace manifest's own settings, minus the ones a
    /// repository-controlled file must not carry.
    pub(super) fn apply_workspace_settings<Sys>(
        &mut self,
        mut settings: WorkspaceSettings,
        base_dir: &Path,
        explicit: &mut ExplicitPaths,
        declared_registries: &mut crate::npmrc_auth::DeclaredRegistries,
        for_self_update: bool,
    ) -> Result<(), LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        // CI detection is process state. A repository-controlled
        // manifest must not be able to turn it off; trusted global
        // config and PNPM_CONFIG_CI are applied in their own layers.
        settings.ci = None;
        settings.state_dir = None;
        settings.scope = None;
        settings.global_dir = None;
        settings.global_bin_dir = None;
        // Noted rather than assigned, so an `enableGlobalVirtualStore` /
        // `virtualStoreDir` set in the global `config.yaml` still counts as
        // "explicitly set" when the workspace yaml leaves it unset.
        explicit.note(&settings);
        settings.substitute_env_untrusted::<Sys>();
        if for_self_update {
            settings.clear_self_update_policy();
        }
        self.workspace_key_issues = settings.key_issues.clone();
        note_declared_registries(declared_registries, &settings);
        collect_explicit_settings(&mut self.explicit_settings, &settings);
        settings.resolve_script_shell(base_dir);
        settings.apply_to(self, base_dir);
        // `overrides` reaches `Config` only from the workspace yaml (the
        // global config.yaml is stripped of the key, and no `PNPM_CONFIG_*`
        // var carries a map), so the `$dep-name` values it may hold are
        // resolved here, against the workspace root's manifest.
        if let Some(overrides) = self.overrides.as_mut() {
            crate::override_version_references::resolve_version_references(overrides, base_dir)?;
        }
        Ok(())
    }

    /// Apply the global layer without changing the discovered workspace directory.
    /// Relative paths use `start_dir`, except `stateDir`: its global-shim trust
    /// records must resolve outside the project being considered for execution.
    /// Workspace-only keys are rejected by [`WorkspaceSettings::load_global`].
    pub(super) fn apply_global_settings<Sys>(
        &mut self,
        global_settings: Option<WorkspaceSettings>,
        explicit: &mut ExplicitPaths,
        declared_registries: &mut crate::npmrc_auth::DeclaredRegistries,
        default_state_dir: &std::path::Path,
        start_dir: &std::path::Path,
    ) where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        // `store_dir_explicit` carries the "did the user set `storeDir`
        // anywhere?" signal through the cascade. Tracked separately
        // from `virtual_store_dir_explicit` because the downstream
        // consumer is different — store_dir's late-stage cross-volume
        // resolution must fire only when the user has *not* pinned a
        // path. See [`crate::store_path::resolve_store_dir`].
        if let Some(mut global_settings) = global_settings {
            note_declared_registries(declared_registries, &global_settings);
            explicit.note(&global_settings);
            collect_explicit_settings(&mut self.explicit_settings, &global_settings);
            let configured_state_dir = global_settings.state_dir.take();
            let saved_workspace_dir = self.workspace_dir.take();
            global_settings.expand_global_dir_home_prefixes::<Sys>();
            global_settings.apply_to(self, start_dir);
            self.workspace_dir = saved_workspace_dir;
            if let Some(configured_state_dir) =
                configured_state_dir.as_deref().filter(|value| !value.is_empty())
            {
                self.state_dir =
                    resolve_configured_state_dir(default_state_dir, configured_state_dir);
            }
        }
    }

    /// Find the workspace root and read its `pnpm-workspace.yaml`.
    pub(super) fn resolve_workspace_yaml<Sys>(
        &self,
        start_dir: &std::path::Path,
    ) -> Result<Option<(PathBuf, Option<WorkspaceSettings>)>, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        // Resolve the workspace dir before reading the project `.npmrc`
        // so subdirectory invocations use the workspace-root config:
        // the workspace dir, falling back to the local prefix.
        //
        // `--ignore-workspace` stops the search outright, which is what
        // makes the flag mean "standalone project": with no workspace dir
        // there is no shared lockfile, no sibling projects, and no
        // `pnpm-workspace.yaml` settings layer. Only the flag reaches
        // this far — see [`Config::ignore_workspace`].
        let env_workspace_dir = Sys::var_os("NPM_CONFIG_WORKSPACE_DIR")
            .or_else(|| Sys::var_os("npm_config_workspace_dir"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let workspace_yaml = if self.ignore_workspace {
            None
        } else if let Some(env_dir) = env_workspace_dir {
            // Env-var path: load yaml directly from the env dir. A
            // missing file is silent, but the re-anchor still fires
            // because the user has explicitly told us where the
            // workspace lives.
            let yaml_path = env_dir.join(WORKSPACE_MANIFEST_FILENAME);
            match fs::read_to_string(&yaml_path) {
                Ok(text) => {
                    let mut settings: WorkspaceSettings =
                        serde_saphyr::from_str(&text).map_err(Box::new).map_err(|source| {
                            LoadWorkspaceYamlError::ParseYaml { path: yaml_path, source }
                        })?;
                    settings.collect_key_issues(&text);
                    Some((env_dir, Some(settings)))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some((env_dir, None)),
                Err(source) => {
                    return Err(LoadWorkspaceYamlError::ReadFile { path: yaml_path, source });
                }
            }
        } else {
            WorkspaceSettings::find_and_load(start_dir)?.map(|(path, settings)| {
                let base_dir = path.parent().unwrap_or(start_dir).to_path_buf();
                (base_dir, Some(settings))
            })
        };
        Ok(workspace_yaml)
    }
}
