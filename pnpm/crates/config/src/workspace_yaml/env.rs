use super::{
    EnvVar,
    GetHomeDir,
    LoadWorkspaceYamlError,
    Path,
    UnresolvedPlaceholders,
    WORKSPACE_MANIFEST_FILENAME,
    WorkspaceSettings,
    has_env_placeholder,
    join_fragment,
    redact_registry_url,
    registries,
    substitute_json_string,
    substitute_optional_inner_string,
    substitute_optional_string,
    substitute_optional_string_map,
    substitute_registry_entries,
};

impl WorkspaceSettings {
    /// Expand `${VAR}` in trusted user-controlled settings, failing with
    /// [`LoadWorkspaceYamlError::ConfigUnresolvedEnvVar`] on a placeholder
    /// with no value and no fallback.
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`](crate::settings::Config).
    pub fn substitute_env_trusted<Sys: EnvVar>(&mut self) -> Result<(), LoadWorkspaceYamlError> {
        let mut unresolved = UnresolvedPlaceholders::fail();
        self.substitute_trusted_values::<Sys>(&mut unresolved);
        unresolved.into_result()
    }

    pub(super) fn substitute_trusted_values<Sys: EnvVar>(
        &mut self,
        unresolved: &mut UnresolvedPlaceholders,
    ) {
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

    /// Expand `${VAR}` in ordinary string settings, but not inside
    /// workspace-controlled request settings: `registry`, `registries`,
    /// `namedRegistries`, `pnprServer`, the proxies, and `userAgent`. The
    /// destinations would let the file choose the host, and `userAgent` would
    /// send that host the variable's value as a request header.
    ///
    /// A registry URL with a placeholder fails with
    /// [`LoadWorkspaceYamlError::RegistryEnvNotExpanded`] and leaves the
    /// settings unchanged: dropping it would resolve the packages it serves
    /// from another registry. Any other request setting with a placeholder is
    /// dropped, and the returned warnings name each one.
    ///
    /// A placeholder with no value and no fallback in an ordinary string
    /// setting fails with [`LoadWorkspaceYamlError::ConfigUnresolvedEnvVar`].
    ///
    /// Call this before [`Self::apply_to`] so expanded values land in
    /// [`Config`](crate::settings::Config) and filtered values do not.
    pub fn substitute_env_untrusted<Sys: EnvVar>(
        &mut self,
    ) -> Result<Vec<String>, LoadWorkspaceYamlError> {
        self.refuse_unexpanded_registries()?;
        self.substitute_untrusted_values::<Sys>(UnresolvedPlaceholders::fail())
    }

    /// [`Self::substitute_env_untrusted`] for a load that leaves out what this
    /// pnpm cannot read: a registry setting it would refuse and a setting with
    /// a placeholder that has no value and no fallback are left out.
    pub(crate) fn substitute_env_untrusted_readable<Sys: EnvVar>(&mut self) -> Vec<String> {
        self.drop_unexpanded_registries();
        self.substitute_untrusted_values::<Sys>(UnresolvedPlaceholders::leave_out())
            .expect("a pass that leaves out unresolved placeholders does not fail")
    }

    fn substitute_untrusted_values<Sys: EnvVar>(
        &mut self,
        mut unresolved: UnresolvedPlaceholders,
    ) -> Result<Vec<String>, LoadWorkspaceYamlError> {
        self.substitute_env_scalars::<Sys>(&mut unresolved);
        unresolved.into_result()?;
        let mut dropped = Vec::new();
        self.drop_unexpanded_request_scalars(&mut dropped);
        Ok(dropped
            .iter()
            .map(|setting| ignored_setting_warning(setting))
            .collect())
    }

    /// Drop the registry settings [`Self::substitute_env_untrusted`] would
    /// refuse, for a load that leaves out what this pnpm refuses.
    fn drop_unexpanded_registries(&mut self) {
        if self.registry.as_deref().is_some_and(has_env_placeholder) {
            self.registry = None;
        }
        if let Some(registries) = self.registries.as_mut() {
            registries::retain_without_env_placeholders(registries, has_env_placeholder);
        }
        if let Some(named_registries) = self.named_registries.as_mut() {
            named_registries.retain(|_, url| !has_env_placeholder(url));
        }
    }

    pub(super) fn refuse_unexpanded_registries(&self) -> Result<(), LoadWorkspaceYamlError> {
        let refuse = |setting: String| {
            Err(LoadWorkspaceYamlError::RegistryEnvNotExpanded {
                setting,
                file: WORKSPACE_MANIFEST_FILENAME.to_string(),
            })
        };
        if self.registry.as_deref().is_some_and(has_env_placeholder) {
            return refuse("registry".to_string());
        }
        if let Some((name, _)) = self.named_registries
            .iter()
            .flatten()
            .find(|(_, url)| has_env_placeholder(url))
        {
            return refuse(format!("namedRegistries['{}']", redact_registry_url(name)));
        }
        self.registries
            .as_ref()
            .map(|registries| {
                registries::refuse_env_placeholder_entries(registries, has_env_placeholder)
            })
            .unwrap_or(Ok(()))
    }

    fn drop_unexpanded_request_scalars(&mut self, dropped: &mut Vec<String>) {
        for (setting, scalar) in [
            ("pnprServer", &mut self.pnpr_server),
            ("httpsProxy", &mut self.https_proxy),
            ("httpProxy", &mut self.http_proxy),
            ("proxy", &mut self.proxy),
            ("userAgent", &mut self.user_agent),
        ] {
            if scalar.as_deref().is_some_and(has_env_placeholder) {
                *scalar = None;
                dropped.push(setting.to_string());
            }
        }
        for (setting, no_proxy) in [("noProxy", &mut self.no_proxy), ("noproxy", &mut self.noproxy)]
        {
            if no_proxy
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .is_some_and(has_env_placeholder)
            {
                *no_proxy = None;
                dropped.push(setting.to_string());
            }
        }
    }

    /// Rewrite a leading `~/` in `storeDir`, `cacheDir`, and the directories of
    /// [`MachineSettings::expand_home_prefixes`](super::MachineSettings::expand_home_prefixes),
    /// into the home directory. A shell expands the tilde before pnpm sees it,
    /// but a configuration file carries it verbatim.
    ///
    /// Call this before [`Self::apply_to`], which would otherwise take the
    /// tilde for an ordinary relative path segment.
    pub(crate) fn expand_home_prefixes<Sys: GetHomeDir>(&mut self) {
        for dir in [&mut self.store_dir, &mut self.cache_dir].into_iter().flatten() {
            if let Some(expanded) = expand_home_prefix::<Sys>(dir) {
                *dir = expanded;
            }
        }
    }

    pub(super) fn substitute_env_scalars<Sys: EnvVar>(
        &mut self,
        unresolved: &mut UnresolvedPlaceholders,
    ) {
        substitute_optional_string::<Sys>(&mut self.tag, unresolved);
        substitute_optional_string::<Sys>(&mut self.access, unresolved);
        substitute_optional_string::<Sys>(&mut self.store_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.modules_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.virtual_store_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.global_virtual_store_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.npmrc_auth_file, unresolved);
        substitute_optional_string::<Sys>(&mut self.lockfile_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.patches_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.cache_dir, unresolved);
        substitute_optional_string::<Sys>(&mut self.local_address, unresolved);
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

fn ignored_setting_warning(setting: &str) -> String {
    format!(
        r#"Ignored "{setting}" in {WORKSPACE_MANIFEST_FILENAME}: environment variables are not expanded in the proxy, pnprServer, and userAgent settings of a file committed to the repository. Write the value literally, or move the setting to the global config.yaml."#,
    )
}

pub(super) fn expand_home_prefix<Sys: GetHomeDir>(dir: &str) -> Option<String> {
    let relative = dir
        .strip_prefix("~/")
        .or_else(|| dir.strip_prefix(r"~\"))?;
    Sys::home_dir()
        .map(|home_dir| join_fragment(&home_dir, relative))?
        .into_os_string()
        .into_string()
        .ok()
}
