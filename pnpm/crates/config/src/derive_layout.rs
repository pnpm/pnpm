use super::{
    Config, EnvVar, EnvVarOs, ExplicitPaths, GLOBAL_LAYOUT_VERSION, GetCurrentDir, GetHomeDir,
    HashMap, LinkProbe, LoadWorkspaceYamlError, NodeLinker, NpmrcAuth, Path, default_pnpm_home_dir,
    esm_node_path_loader,
};

impl Config {
    pub(crate) fn default_lockfile_dir_for<'a>(&'a self, project_dir: &'a Path) -> &'a Path {
        if self.shared_workspace_lockfile {
            self.workspace_dir.as_deref().unwrap_or(project_dir)
        } else {
            project_dir
        }
    }

    /// Whether pinning `dir` leaves the layout as it is without a pin.
    /// A pin inside a workspace that keeps per-project lockfiles is never
    /// default: it makes the workspace share one lockfile.
    pub(crate) fn lockfile_dir_is_default(&self, dir: &Path, project_dir: &Path) -> bool {
        if self.workspace_dir.is_some() && !self.shared_workspace_lockfile {
            return false;
        }
        let dir = pnpm_fs::lexical_normalize(dir);
        let default = pnpm_fs::lexical_normalize(self.default_lockfile_dir_for(project_dir));
        if dir == default {
            return true;
        }
        #[cfg(windows)]
        {
            dir.to_string_lossy()
                .eq_ignore_ascii_case(&default.to_string_lossy())
                && same_file::is_same_file(dir, default).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    pub fn refresh_node_path_after_lockfile_dir_change<Sys>(
        &mut self,
        previous_node_path_dirs: [&Path; 2],
        hook_node_path: Option<Option<String>>,
        hook_node_options: Option<Option<String>>,
    ) where
        Sys: EnvVar,
    {
        if hook_node_path.is_none() {
            self.remove_previous_global_virtual_store_node_path(previous_node_path_dirs);
        }
        if hook_node_path.is_none()
            && cfg!(any(unix, target_os = "wasi"))
            && self.prefer_symlinked_executables == Some(true)
        {
            remove_node_paths(&mut self.extra_env, previous_node_path_dirs);
            let hidden_modules_dir =
                pnpm_fs::lexical_normalize(&self.install_state_dir.join("node_modules"));
            append_node_path(&mut self.extra_env, &hidden_modules_dir);
        }
        self.apply_global_virtual_store_node_path::<Sys>();
        if let Some(node_path) = hook_node_path {
            set_extra_env_value(&mut self.extra_env, "NODE_PATH", node_path);
        }
        if let Some(node_options) = hook_node_options {
            let previous_node_options = self.extra_env.get("NODE_OPTIONS").map(String::as_str);
            let node_options = match node_options {
                Some(node_options) => Some(esm_node_path_loader::keep_esm_node_path_loader_option(
                    &node_options,
                    previous_node_options,
                )),
                None if previous_node_options.is_some_and(|value| {
                    value.contains(esm_node_path_loader::esm_node_path_loader_import_flag())
                }) =>
                {
                    Some(esm_node_path_loader::esm_node_path_loader_import_flag().to_string())
                }
                None => None,
            };
            set_extra_env_value(&mut self.extra_env, "NODE_OPTIONS", node_options);
        }
    }

    fn remove_previous_global_virtual_store_node_path(&mut self, previous_dirs: [&Path; 2]) {
        if self.enable_global_virtual_store
            && self.extend_node_path
            && self.node_linker == NodeLinker::Isolated
        {
            remove_node_paths(&mut self.extra_env, previous_dirs);
        }
    }

    /// The directory layout and environment every spawned process sees,
    /// derived once every source has had its say.
    pub(super) fn apply_layout_derivations<Sys>(&mut self)
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        // Resolve the global install directories:
        // `globalPkgDir = (globalDir ?? <pnpm-home>/global)/v11` and
        // `bin = globalBinDir ?? <pnpm-home>/bin`.
        let pnpm_home_dir = default_pnpm_home_dir::<Sys>();
        let global_dir_root = self.global_dir
            .clone()
            .or_else(|| {
                pnpm_home_dir
                    .as_ref()
                    .map(|home| home.join("global"))
            });
        self.global_pkg_dir = global_dir_root.map(|root| root.join(GLOBAL_LAYOUT_VERSION));
        self.global_bin = self.global_bin_dir
            .clone()
            .or_else(|| {
                pnpm_home_dir
                    .as_ref()
                    .map(|home| home.join("bin"))
            });

        // Inside a workspace, scripts and `pnpm exec` also get the
        // workspace root's modules `.bin` on PATH, pnpm's
        // `extraBinPaths = [join(workspaceDir, modulesDir, '.bin')]`.
        // The root is a project like any other, so a `packageConfigs`
        // entry naming it moves those executables too.
        self.extra_bin_paths = self.workspace_root_bin_paths();

        // With `preferSymlinkedExecutables`, `.bin` entries are plain
        // symlinks with no shim to carry a `NODE_PATH` block, so the
        // resolution help moves to the environment: expose the virtual
        // store's hidden `node_modules` to every spawned child process.
        // `install_state_dir` is already anchored at the workspace root
        // by the re-anchor above — pnpm builds this from
        // `lockfileDir ?? dir` to the same effect
        // (pnpm/pnpm#13912). Unix only, like pnpm; and only an explicit
        // `true` fires.
        if cfg!(any(unix, target_os = "wasi")) && self.prefer_symlinked_executables == Some(true) {
            let hidden_modules_dir =
                pnpm_fs::lexical_normalize(&self.install_state_dir.join("node_modules"));
            append_node_path(&mut self.extra_env, &hidden_modules_dir);
        }
        self.apply_prefer_symlinked_executables_derivation();

        self.apply_global_virtual_store_node_path::<Sys>();
    }

    /// With a global virtual store, package directories live outside the
    /// project, so Node's upward `node_modules` walk from their real paths
    /// never reaches the project's hoisted `node_modules` or root
    /// `node_modules`. Expose both through `NODE_PATH` for every child
    /// process pnpm spawns, and register the ESM loader that restores
    /// `NODE_PATH` lookups for ESM imports. Mirrors the pnpm config reader
    /// (`pnpm11/config/reader/src/index.ts`).
    pub fn apply_global_virtual_store_node_path<Sys>(&mut self)
    where
        Sys: EnvVar,
    {
        if !(self.enable_global_virtual_store
            && self.extend_node_path
            && self.node_linker == NodeLinker::Isolated)
        {
            return;
        }
        let path_delimiter = if cfg!(windows) { ';' } else { ':' };
        let mut node_paths: Vec<String> = self.extra_env
            .get("NODE_PATH")
            .map(|value| {
                value
                    .split(path_delimiter)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        for dir in [self.install_state_dir.join("node_modules"), self.modules_dir.clone()] {
            // `install_state_dir` is built by joining a multi-segment
            // literal, which keeps `/` separators on Windows; normalize
            // so NODE_PATH carries native separators like the shims do.
            let dir = pnpm_fs::lexical_normalize(&dir).display().to_string();
            if !node_paths.contains(&dir) {
                node_paths.push(dir);
            }
        }
        self.extra_env.insert(
            "NODE_PATH".to_string(),
            node_paths.join(&path_delimiter.to_string()),
        );
        let node_options = self.extra_env
            .get("NODE_OPTIONS")
            .cloned()
            .or_else(|| Sys::var("NODE_OPTIONS"));
        self.extra_env.insert(
            "NODE_OPTIONS".to_string(),
            esm_node_path_loader::add_esm_node_path_loader_option(node_options.as_deref()),
        );
    }

    /// Anchor the lockfile-relative paths, build the auth headers, and settle
    /// the store and virtual-store locations.
    pub(super) fn apply_store_derivations<Sys>(
        &mut self,
        explicit: ExplicitPaths,
        npmrc_auth: &mut NpmrcAuth,
        start_dir: &std::path::Path,
    ) -> Result<(), LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        if !self.explicit_settings.contains_key("lockfile") {
            self.lockfile = self.package_lock;
        }

        let lockfile_dir_is_default = self.lockfile_dir
            .as_ref()
            .is_some_and(|lockfile_dir| self.lockfile_dir_is_default(lockfile_dir, start_dir));
        if lockfile_dir_is_default {
            self.lockfile_dir = None;
        }

        // A pinned `lockfileDir` moves the root `node_modules` and the
        // virtual store with it. Applied after every source has had its
        // say so the anchor uses the final value, and before the
        // global-virtual-store derivation, which separates the shared
        // package store from the project's internal directory.
        if let Some(lockfile_dir) = self.lockfile_dir.clone() {
            self.anchor_lockfile_paths(&lockfile_dir);
        } else if self.explicit_settings.contains_key("modulesDir") {
            self.follow_modules_dir_with_install_state_dir();
        }

        // Build the per-URI auth-header lookup. Credentials were already
        // pinned to their source file's registry by `rescope_unscoped`,
        // so this is independent of the final `config.registry` (which
        // yaml may have overridden) — the security boundary holds even
        // when the workspace points the default registry elsewhere.
        std::mem::take(npmrc_auth).build_auth_headers(self)?;

        // Re-resolve `store_dir` against the project's volume when no
        // explicit source (global config.yaml, pnpm-workspace.yaml,
        // `PNPM_CONFIG_STORE_DIR`) set it.
        if explicit.store_dir {
            self.store_dir_placement_skipped = false;
        } else {
            self.resolve_default_store_dir::<Sys>(start_dir);
        }

        // Derive `global_virtual_store_dir` last so it sees the final
        // `store_dir` / `virtualStoreDir` after yaml has been
        // applied. See [`Self::apply_global_virtual_store_derivation`].
        self.apply_global_virtual_store_derivation(
            explicit.virtual_store_dir,
            explicit.global_virtual_store_dir,
        );

        self.apply_git_branch_lockfile_derivation::<Sys>();
        self.apply_shamefully_hoist_derivation();
        self.apply_virtual_store_only_derivation();
        Ok(())
    }

    /// Make `workspace_dir` the workspace root of a config that was loaded
    /// outside any workspace, for a `pnpm-workspace.yaml` created after
    /// loading. The layout that follows the workspace root follows it too.
    pub fn anchor_to_created_workspace(
        &mut self,
        workspace_dir: std::path::PathBuf,
        package_patterns: Vec<String>,
    ) {
        self.workspace_dir = Some(workspace_dir);
        self.workspace_package_patterns = Some(package_patterns);
        self.extra_bin_paths = self.workspace_root_bin_paths();
    }

    fn workspace_root_bin_paths(&self) -> Vec<std::path::PathBuf> {
        self.workspace_dir
            .as_deref()
            .map_or_else(Vec::new, |dir| {
                let root_name = self
                    .applies_package_configs()
                    .then(|| pnpm_workspace::read_project_name(dir))
                    .flatten();
                vec![self.project_modules_dir(dir, root_name.as_deref()).join(".bin")]
            })
    }

    /// The `nodeOptions` setting to export as `NODE_OPTIONS`. An empty value
    /// only overrides lower-priority settings, so it yields `None` and the
    /// child keeps the `NODE_OPTIONS` it inherits.
    pub fn exported_node_options(&self) -> Option<&str> {
        self.node_options
            .as_deref()
            .filter(|value| !value.is_empty())
    }

    /// [`Config::extra_env`] with the `nodeOptions` setting applied as
    /// `NODE_OPTIONS`, preserving the ESM `NODE_PATH` loader flag the
    /// `extra_env` carries under a global virtual store.
    pub fn extra_env_with_node_options(&self) -> HashMap<String, String> {
        let mut extra_env = self.extra_env.clone();
        if let Some(node_options) = self.exported_node_options() {
            let node_options = esm_node_path_loader::keep_esm_node_path_loader_option(
                node_options,
                self.extra_env.get("NODE_OPTIONS").map(String::as_str),
            );
            extra_env.insert("NODE_OPTIONS".to_string(), node_options);
        }
        extra_env
    }
}

fn append_node_path(extra_env: &mut HashMap<String, String>, dir: &Path) {
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    let dir = pnpm_fs::lexical_normalize(dir).display().to_string();
    let mut paths = extra_env
        .get("NODE_PATH")
        .map(|value| {
            value
                .split(delimiter)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !paths.contains(&dir) {
        paths.push(dir);
    }
    extra_env.insert("NODE_PATH".to_string(), paths.join(&delimiter.to_string()));
}

fn remove_node_paths(extra_env: &mut HashMap<String, String>, dirs: [&Path; 2]) {
    let Some(node_path) = extra_env.get("NODE_PATH").cloned() else { return };
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    let removed = dirs.map(|dir| pnpm_fs::lexical_normalize(dir).display().to_string());
    let node_paths = node_path
        .split(delimiter)
        .filter(|path| !removed.iter().any(|dir| dir == path))
        .collect::<Vec<_>>();
    set_extra_env_value(
        extra_env,
        "NODE_PATH",
        (!node_paths.is_empty()).then(|| node_paths.join(&delimiter.to_string())),
    );
}

fn set_extra_env_value(extra_env: &mut HashMap<String, String>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        extra_env.insert(key.to_string(), value);
    } else {
        extra_env.remove(key);
    }
}
