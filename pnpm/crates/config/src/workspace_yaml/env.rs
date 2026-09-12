use super::{
    EnvVar, GetHomeDir, Path, WorkspaceSettings, has_env_placeholder, join_fragment, registries,
    substitute_json_string, substitute_optional_inner_string, substitute_optional_string,
    substitute_optional_string_map, substitute_registry_entries,
};

impl WorkspaceSettings {
    /// Expand `${VAR}` in trusted user-controlled settings.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`](crate::settings::Config).
    pub fn substitute_env_trusted<Sys: EnvVar>(&mut self) {
        self.substitute_env_scalars::<Sys>();
        substitute_optional_string::<Sys>(&mut self.pnpr_server);
        substitute_optional_string::<Sys>(&mut self.registry);
        substitute_optional_string::<Sys>(&mut self.https_proxy);
        substitute_optional_string::<Sys>(&mut self.http_proxy);
        substitute_optional_string::<Sys>(&mut self.proxy);
        substitute_json_string::<Sys>(&mut self.no_proxy);
        substitute_json_string::<Sys>(&mut self.noproxy);
        substitute_registry_entries::<Sys>(&mut self.registries);
        substitute_optional_string_map::<Sys>(&mut self.named_registries);
    }

    /// Expand `${VAR}` in ordinary string settings, but drop
    /// placeholders inside workspace-controlled request-destination
    /// fields. Scalar strings still have `${VAR}` expanded, while
    /// `registry`, `registries`, `namedRegistries`, and `pnprServer`
    /// are filtered instead of expanding environment variables into
    /// request URLs.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`](crate::settings::Config) and filtered values do not.
    pub fn substitute_env_untrusted<Sys: EnvVar>(&mut self) {
        self.substitute_env_scalars::<Sys>();

        if self.registry.as_deref().is_some_and(has_env_placeholder) {
            self.registry = None;
        }
        if let Some(registries) = self.registries.as_mut() {
            registries::retain_without_env_placeholders(registries, has_env_placeholder);
        }
        if let Some(named_registries) = self.named_registries.as_mut() {
            named_registries.retain(|_, value| !has_env_placeholder(value));
        }

        if self.pnpr_server.as_deref().is_some_and(has_env_placeholder) {
            self.pnpr_server = None;
        }
        for proxy in [&mut self.https_proxy, &mut self.http_proxy, &mut self.proxy] {
            if proxy.as_deref().is_some_and(has_env_placeholder) {
                *proxy = None;
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
    }

    /// Rewrite a leading `~/` in `globalDir` / `globalBinDir` into the home
    /// directory, as pnpm's `transformGlobalDirKeys` does. A shell expands
    /// the tilde before `pnpm config set` sees it, but a hand-written
    /// `config.yaml` carries it verbatim.
    ///
    /// Call this before [`Self::apply_to`], which would otherwise take the
    /// tilde for an ordinary relative path segment.
    pub(crate) fn expand_global_dir_home_prefixes<Sys: GetHomeDir>(&mut self) {
        for dir in [&mut self.global_dir, &mut self.global_bin_dir] {
            let Some(relative) = dir
                .as_deref()
                .and_then(|dir| dir.strip_prefix("~/").or_else(|| dir.strip_prefix(r"~\")))
            else {
                continue;
            };
            if let Some(expanded) = Sys::home_dir()
                .map(|home_dir| join_fragment(&home_dir, relative))
                .and_then(|expanded| expanded.into_os_string().into_string().ok())
            {
                *dir = Some(expanded);
            }
        }
    }

    pub(super) fn substitute_env_scalars<Sys: EnvVar>(&mut self) {
        substitute_optional_string::<Sys>(&mut self.scope);
        substitute_optional_string::<Sys>(&mut self.store_dir);
        substitute_optional_string::<Sys>(&mut self.state_dir);
        substitute_optional_string::<Sys>(&mut self.modules_dir);
        substitute_optional_string::<Sys>(&mut self.virtual_store_dir);
        substitute_optional_string::<Sys>(&mut self.global_virtual_store_dir);
        substitute_optional_string::<Sys>(&mut self.global_dir);
        substitute_optional_string::<Sys>(&mut self.global_bin_dir);
        substitute_optional_string::<Sys>(&mut self.user_agent);
        substitute_optional_string::<Sys>(&mut self.npmrc_auth_file);
        substitute_optional_string::<Sys>(&mut self.lockfile_dir);
        substitute_optional_string::<Sys>(&mut self.patches_dir);
        substitute_optional_string::<Sys>(&mut self.cache_dir);
        substitute_optional_inner_string::<Sys>(&mut self.script_shell);
        substitute_optional_inner_string::<Sys>(&mut self.node_options);
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
