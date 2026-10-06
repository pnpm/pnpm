use super::{
    EnvVar, FirstUnresolved, GetHomeDir, LoadWorkspaceYamlError, Path, WorkspaceSettings,
    has_env_placeholder, join_fragment, registries, substitute_json_string,
    substitute_optional_inner_string, substitute_optional_string, substitute_optional_string_map,
    substitute_registry_entries,
};

impl WorkspaceSettings {
    /// Expand `${VAR}` in trusted user-controlled settings, failing on a
    /// placeholder with no value and no fallback.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`](crate::settings::Config).
    pub fn substitute_env_trusted<Sys: EnvVar>(&mut self) -> Result<(), LoadWorkspaceYamlError> {
        let mut unresolved = FirstUnresolved::default();
        self.substitute_trusted_values::<Sys>(&mut unresolved);
        unresolved.into_result()
    }

    /// [`Self::substitute_env_trusted`] for settings read from `PNPM_CONFIG_*`
    /// variables, which expands a placeholder the environment cannot resolve
    /// to an empty string.
    pub fn substitute_env_trusted_lossy<Sys: EnvVar>(&mut self) {
        self.substitute_trusted_values::<Sys>(&mut FirstUnresolved::default());
    }

    fn substitute_trusted_values<Sys: EnvVar>(&mut self, unresolved: &mut FirstUnresolved) {
        self.substitute_env_scalars::<Sys>(unresolved);
        substitute_optional_string::<Sys>(&mut self.user_agent, unresolved);
        substitute_optional_string::<Sys>(&mut self.pnpr_server, unresolved);
        substitute_optional_string::<Sys>(&mut self.registry, unresolved);
        substitute_optional_string::<Sys>(&mut self.https_proxy, unresolved);
        substitute_optional_string::<Sys>(&mut self.http_proxy, unresolved);
        substitute_optional_string::<Sys>(&mut self.proxy, unresolved);
        substitute_json_string::<Sys>(&mut self.no_proxy, unresolved);
        substitute_json_string::<Sys>(&mut self.noproxy, unresolved);
        substitute_registry_entries::<Sys>(&mut self.registries, unresolved);
        substitute_optional_string_map::<Sys>(&mut self.named_registries, unresolved);
    }

    /// Expand `${VAR}` in ordinary string settings, but drop
    /// placeholders inside workspace-controlled request settings.
    /// Scalar strings still have `${VAR}` expanded, while `registry`,
    /// `registries`, `namedRegistries`, `pnprServer`, the proxies, and
    /// `userAgent` are filtered instead: the destinations would let the
    /// file choose the host, and `userAgent` would send that host the
    /// variable's value as a request header.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`](crate::settings::Config) and filtered values do not.
    pub fn substitute_env_untrusted<Sys: EnvVar>(&mut self) -> Result<(), LoadWorkspaceYamlError> {
        let mut unresolved = FirstUnresolved::default();
        self.substitute_env_scalars::<Sys>(&mut unresolved);
        unresolved.into_result()?;

        if self.registry.as_deref().is_some_and(has_env_placeholder) {
            self.registry = None;
        }
        if let Some(registries) = self.registries.as_mut() {
            registries::retain_without_env_placeholders(registries, has_env_placeholder);
        }
        if let Some(named_registries) = self.named_registries.as_mut() {
            named_registries.retain(|_, value| !has_env_placeholder(value));
        }

        for scalar in [
            &mut self.pnpr_server,
            &mut self.https_proxy,
            &mut self.http_proxy,
            &mut self.proxy,
            &mut self.user_agent,
        ] {
            if scalar.as_deref().is_some_and(has_env_placeholder) {
                *scalar = None;
            }
        }
        for no_proxy in [&mut self.no_proxy, &mut self.noproxy] {
            if no_proxy
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .is_some_and(has_env_placeholder)
            {
                *no_proxy = None;
            }
        }
        Ok(())
    }

    /// Rewrite a leading `~/` in `storeDir`, `globalDir`, and `globalBinDir`
    /// into the home directory. A shell expands the tilde before pnpm sees it,
    /// but a configuration file carries it verbatim.
    ///
    /// Call this before [`Self::apply_to`], which would otherwise take the
    /// tilde for an ordinary relative path segment.
    pub(crate) fn expand_home_prefixes<Sys: GetHomeDir>(&mut self) {
        for dir in [&mut self.store_dir, &mut self.global_dir, &mut self.global_bin_dir] {
            let Some(relative) = dir
                .as_deref()
                .and_then(|dir| {
                    dir.strip_prefix("~/")
                        .or_else(|| dir.strip_prefix(r"~\"))
                })
            else {
                continue;
            };
            if let Some(expanded) = Sys::home_dir()
                .map(|home_dir| join_fragment(&home_dir, relative))
                .and_then(|expanded| {
                    expanded
                        .into_os_string()
                        .into_string()
                        .ok()
                })
            {
                *dir = Some(expanded);
            }
        }
    }

    pub(super) fn substitute_env_scalars<Sys: EnvVar>(&mut self, unresolved: &mut FirstUnresolved) {
        substitute_optional_string::<Sys>(&mut self.scope, unresolved);
        substitute_optional_string::<Sys>(&mut self.store_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.state_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.modules_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.virtual_store_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.global_virtual_store_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.global_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.global_bin_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.npmrc_auth_file, unresolved);
        substitute_optional_string::<Sys>(&mut self.lockfile_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.patches_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.cache_dir, unresolved);
        substitute_optional_inner_string::<Sys>(&mut self.script_shell, unresolved);
        substitute_optional_inner_string::<Sys>(&mut self.node_options, unresolved);
    }

    /// Resolve a path-like `scriptShell` against the workspace root, the
    /// way pnpm does for the settings of `pnpm-workspace.yaml` and for no
    /// other source: a relative shell path in the global config file, in
    /// `PNPM_CONFIG_SCRIPT_SHELL`, or in an `updateConfig` hook's output
    /// stays as written. A bare command name (`bash`) is left for `PATH`
    /// lookup, and an absolute path is kept.
    ///
    /// Call this after environment substitution and before
    /// [`Self::apply_to`], which copies the value verbatim.
    pub fn resolve_script_shell(&mut self, workspace_dir: &Path) {
        let Some(Some(script_shell)) = self.script_shell.as_mut() else { return };
        // `has_root` rather than `is_absolute`: Node's win32 `isAbsolute`
        // accepts a rooted path without a drive (`\tools\bash.exe`), which
        // Rust's `is_absolute` rejects. On POSIX the two agree.
        if Path::new(script_shell.as_str()).has_root() || !script_shell.contains(['/', '\\']) {
            return;
        }
        *script_shell = join_fragment(workspace_dir, script_shell).to_string_lossy().into_owned();
    }
}
