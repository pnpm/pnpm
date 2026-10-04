use super::{
    Config, EnvVar, GetCurrentDir, GetHomeDir, GitHost, HashMap, HoistPatterns, LinkProbe,
    Lockfile, NodeLinker, Path, StoreDir, StoreRelocation, WantedLockfileSelection,
    WorkspaceSettings, collect_explicit_settings, create_matcher, default_store_dir,
    esm_node_path_loader, get_branches_containing_head, get_current_branch, store_path,
};

impl Config {
    /// Settle the two directories a global virtual store separates:
    ///
    /// - [`Self::global_virtual_store_dir`], where packages are linked under
    ///   a global virtual store. It is the `globalVirtualStoreDir` setting
    ///   when `global_virtual_store_dir_explicit`, otherwise the
    ///   [`Self::configured_virtual_store_dir`] when
    ///   `virtual_store_dir_explicit` and the global virtual store is on,
    ///   otherwise `<store_dir>/links`.
    /// - [`Self::install_state_dir`], which stays at `<modules_dir>/.pnpm`
    ///   under a global virtual store, so projects sharing that store keep
    ///   their own current lockfile and hidden hoisted modules. Without a
    ///   global virtual store it is the virtual store itself, at the
    ///   configured `virtualStoreDir` when one is set.
    ///
    /// pnpm instead rewrites `virtualStoreDir` to the global virtual store
    /// and keeps the state in `installStateDir`. Pacquet's
    /// [`Self::virtual_store_dir`] gives the same answer as pnpm's
    /// `virtualStoreDir`.
    ///
    /// The derivation reads only settled inputs, so it runs again whenever
    /// one of them changes: `--store-dir`, `updateConfig` hooks, and store
    /// placement.
    pub fn apply_global_virtual_store_derivation(
        &mut self,
        virtual_store_dir_explicit: bool,
        global_virtual_store_dir_explicit: bool,
    ) {
        self.apply_cas_layout();
        if !global_virtual_store_dir_explicit {
            self.global_virtual_store_dir = match &self.configured_virtual_store_dir {
                Some(dir) if self.enable_global_virtual_store && virtual_store_dir_explicit => {
                    dir.clone()
                }
                _ => self.store_dir.links(),
            };
        }
        if self.enable_global_virtual_store {
            self.install_state_dir = self.modules_dir.join(".pnpm");
        } else if virtual_store_dir_explicit && let Some(dir) = &self.configured_virtual_store_dir {
            self.install_state_dir.clone_from(dir);
        }
    }

    /// Apply a resolved `virtualStoreDir` setting. It is retained in
    /// [`Self::configured_virtual_store_dir`] for
    /// [`Self::apply_global_virtual_store_derivation`], which moves
    /// [`Self::install_state_dir`] back into the modules directory under a
    /// global virtual store.
    pub fn set_virtual_store_dir(&mut self, dir: std::path::PathBuf) {
        self.configured_virtual_store_dir = Some(dir.clone());
        self.install_state_dir = dir;
    }

    /// The directory owning the `pnpm-lock.yaml` that covers
    /// `project_dir`: the pinned [`lockfile_dir`], else the workspace root
    /// when the workspace shares one lockfile, else the project itself.
    ///
    /// Mirrors pnpm's `lockfileDir ?? dir`, whose config reader has
    /// already defaulted `lockfileDir` to `workspaceDir` for a shared
    /// workspace lockfile.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    #[must_use]
    pub fn lockfile_dir_for<'a>(&'a self, project_dir: &'a Path) -> &'a Path {
        self.lockfile_dir
            .as_deref()
            .unwrap_or_else(|| {
                if self.shared_workspace_lockfile {
                    self.workspace_dir.as_deref().unwrap_or(project_dir)
                } else {
                    project_dir
                }
            })
    }

    /// Whether one `pnpm-lock.yaml` covers every project the command
    /// touches. The `sharedWorkspaceLockfile` setting, which an explicit
    /// [`lockfile_dir`] overrides: pinning the lockfile to one directory
    /// *is* the shared layout, and pnpm's recursive dispatch routes such
    /// a run through its shared-lockfile branch whatever the setting
    /// says.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    #[must_use]
    pub fn shares_one_lockfile(&self) -> bool {
        self.lockfile_dir.is_some() || self.shared_workspace_lockfile
    }

    /// How a walk that leaves out a dependency group classifies the lockfile's
    /// optional-peer edges.
    #[must_use]
    pub fn peer_edge_options(&self) -> pnpm_lockfile::PeerEdgeOptions {
        pnpm_lockfile::PeerEdgeOptions {
            resolve_peers_from_workspace_root: self.resolve_peers_from_workspace_root,
        }
    }

    /// pnpm's `rootProjectManifestDir`: where the root `package.json`,
    /// the config dependencies (`node_modules/.pnpm-config`), and the
    /// pnpmfile a command reads live — `lockfileDir ?? workspaceDir ??
    /// dir`.
    ///
    /// Not the directory settings are *written* back to: `pnpm-workspace.yaml`
    /// stays at [`workspace_dir`] when there is one.
    ///
    /// [`workspace_dir`]: Self::workspace_dir
    #[must_use]
    pub fn root_project_manifest_dir<'a>(&'a self, dir: &'a Path) -> &'a Path {
        self.lockfile_dir
            .as_deref()
            .or(self.workspace_dir.as_deref())
            .unwrap_or(dir)
    }

    /// Pin [`lockfile_dir`] to `dir` and re-anchor the paths that follow
    /// the lockfile with it.
    ///
    /// `dir` is normalized first: importer ids are a lexical path diff
    /// against it, so an unnormalized `<workspace>/..` would not name the
    /// project it points at.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    pub fn pin_lockfile_dir(&mut self, dir: &Path) {
        let dir = pnpm_fs::lexical_normalize(dir);
        self.anchor_lockfile_paths(&dir);
        self.lockfile_dir = Some(dir);
    }

    /// Re-anchor the paths pnpm resolves against `lockfileDir` — the root
    /// `node_modules` and the virtual store — onto `dir`.
    ///
    /// An explicitly configured `modulesDir` / `virtualStoreDir` keeps its
    /// raw value (recovered from [`explicit_settings`]) and is re-resolved
    /// against `dir`, so a multi-component or absolute setting keeps its
    /// full shape — [`Path::join`] leaves an absolute value absolute.
    /// Under a global virtual store a `virtualStoreDir` names the store's
    /// root, so it is not re-anchored.
    ///
    /// [`explicit_settings`]: Self::explicit_settings
    pub fn anchor_lockfile_paths(&mut self, dir: &Path) {
        self.modules_dir = match self.explicit_settings
            .get("modulesDir")
            .and_then(serde_json::Value::as_str)
        {
            Some(raw) => dir.join(raw),
            None => {
                dir.join(if self.node_linker == NodeLinker::Cas { ".pnpm" } else { "node_modules" })
            }
        };
        match self.explicit_settings.get("virtualStoreDir").and_then(serde_json::Value::as_str) {
            Some(raw) if !self.enable_global_virtual_store => {
                self.set_virtual_store_dir(dir.join(raw));
            }
            _ => self.follow_modules_dir_with_install_state_dir(),
        }
    }

    /// Put [`Self::install_state_dir`] at `<modules_dir>/.pnpm`, pnpm's
    /// default, unless `virtualStoreDir` is set and the global virtual store
    /// is off.
    pub(crate) fn follow_modules_dir_with_install_state_dir(&mut self) {
        if self.enable_global_virtual_store
            || !self.explicit_settings.contains_key("virtualStoreDir")
        {
            self.install_state_dir = self.modules_dir.join(".pnpm");
        }
    }

    /// [`Self::anchor_lockfile_paths`] plus the `packageConfigs` entry
    /// declared for `project_name`, for the per-project installs of a
    /// workspace whose projects keep their own lockfiles.
    ///
    /// A nameless project, and one the setting does not name, keep the
    /// workspace-wide settings. Callers pass the name rather than the
    /// config reading it, so a workspace-scale run spends no manifest
    /// read here: the plans that install several projects already hold
    /// every manifest they discovered.
    pub fn anchor_dedicated_project(&mut self, project_dir: &Path, project_name: Option<&str>) {
        self.anchor_lockfile_paths(project_dir);
        let Some(project_config) = project_name
            .and_then(|name| self.package_configs.as_ref()?.get(name))
            .cloned()
        else {
            return;
        };
        project_config.apply_to(self, project_dir);
    }

    /// The path, relative to a project's directory, of the modules
    /// directory an install gives every project. It is `node_modules`
    /// unless `modulesDir` is configured, and the joins that build a
    /// project's modules or `.bin` path must use it rather than the
    /// literal `node_modules`.
    ///
    /// A relative `modulesDir` such as `www/modules` carries over whole,
    /// joined onto every project. A value that climbs out of
    /// the project (`..`) or is absolute cannot, so only its last
    /// component does.
    pub fn modules_dir_name(&self) -> &std::ffi::OsStr {
        self.explicit_settings
            .get("modulesDir")
            .and_then(serde_json::Value::as_str)
            .and_then(|raw| project_relative_modules_dir(raw, &self.modules_dir))
            .map(Path::as_os_str)
            .or_else(|| self.modules_dir.file_name())
            .unwrap_or_else(|| std::ffi::OsStr::new("node_modules"))
    }

    /// The directory [`Config::modules_dir`] was resolved against: the
    /// lockfile directory, onto which the install places every importer.
    #[must_use]
    pub fn modules_dir_anchor(&self) -> Option<&Path> {
        self.modules_dir
            .ancestors()
            .nth(Path::new(self.modules_dir_name()).components().count())
    }

    /// Whether a `packageConfigs` entry can still change a project's
    /// layout. An entry reaches its project through the install that
    /// project owns, so a workspace sharing one lockfile applies none of
    /// them, and a command reading a project's directories back has to
    /// draw the line in the same place.
    #[must_use]
    pub fn applies_package_configs(&self) -> bool {
        self.package_configs.is_some() && !self.shares_one_lockfile()
    }

    /// [`Self::modules_dir_name`] for one project, which the
    /// `packageConfigs` entry naming it may point elsewhere. The name
    /// the install gave that project.
    ///
    /// `project_name` is what [`Self::anchor_dedicated_project`] takes,
    /// and for the same reason: callers hold a manifest they already
    /// read rather than reading one here.
    #[must_use]
    pub fn modules_dir_name_for(
        &self,
        project_dir: &Path,
        project_name: Option<&str>,
    ) -> std::borrow::Cow<'_, std::ffi::OsStr> {
        self.applies_package_configs()
            .then(|| {
                self.package_configs
                    .as_ref()?
                    .get(project_name?)?
                    .modules_dir_name_for(project_dir)
            })
            .flatten()
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(self.modules_dir_name()))
    }

    /// The modules directory an install gives the project at
    /// `project_dir`: the `packageConfigs` entry naming it, else the
    /// configured `modulesDir`, resolved against `project_dir` and
    /// lexically normalized. Unlike [`Self::modules_dir_name_for`] it
    /// keeps a multi-component or absolute setting whole, as pnpm's
    /// `pathAbsolute` does.
    #[must_use]
    pub fn project_modules_dir(
        &self,
        project_dir: &Path,
        project_name: Option<&str>,
    ) -> std::path::PathBuf {
        let modules_dir = self
            .applies_package_configs()
            .then(|| {
                self.package_configs
                    .as_ref()?
                    .get(project_name?)?
                    .modules_dir_for(project_dir)
            })
            .flatten()
            .unwrap_or_else(|| {
                let raw = self.explicit_settings
                    .get("modulesDir")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(if self.node_linker == NodeLinker::Cas {
                        ".pnpm"
                    } else {
                        "node_modules"
                    });
                project_dir.join(raw)
            });
        pnpm_fs::lexical_normalize(&modules_dir)
    }

    /// Put `<project_dir>/<modules_dir_name>` first on the `NODE_PATH` of
    /// `env`, the environment of that project's scripts and commands, when
    /// it is a custom modules directory and the project's executables are
    /// symlinks, which have no shim to carry the entry. The rest of
    /// `NODE_PATH` is the one `env` sets, or else the inherited one.
    pub fn prepend_project_node_path<Sys: EnvVar>(
        &self,
        env: &mut HashMap<String, String>,
        project_dir: &Path,
        modules_dir_name: &std::ffi::OsStr,
    ) {
        let symlinked =
            cfg!(any(unix, target_os = "wasi")) && self.prefer_symlinked_executables == Some(true);
        if !symlinked || !self.extend_node_path || modules_dir_name == "node_modules" {
            return;
        }
        let project_node_path = project_dir
            .join(modules_dir_name)
            .display()
            .to_string();
        if project_node_path.contains(':') {
            return;
        }
        let rest = env
            .get("NODE_PATH")
            .cloned()
            .or_else(|| Sys::var("NODE_PATH"))
            .unwrap_or_default();
        let node_path = std::iter::once(project_node_path.as_str())
            .chain(
                rest.split(':')
                    .filter(|entry| !entry.is_empty() && *entry != project_node_path),
            )
            .collect::<Vec<_>>()
            .join(":");
        env.insert("NODE_PATH".to_string(), node_path);
    }

    /// [`Config::extra_env`] with the `nodeOptions` setting applied as
    /// `NODE_OPTIONS`, preserving the ESM `NODE_PATH` loader flag the
    /// `extra_env` carries under a global virtual store.
    pub fn extra_env_with_node_options(&self) -> HashMap<String, String> {
        let mut extra_env = self.extra_env.clone();
        if let Some(node_options) = &self.node_options {
            let node_options = esm_node_path_loader::keep_esm_node_path_loader_option(
                node_options,
                self.extra_env.get("NODE_OPTIONS").map(String::as_str),
            );
            extra_env.insert("NODE_OPTIONS".to_string(), node_options);
        }
        extra_env
    }

    /// Clear both hoist patterns when [`virtual_store_only`] is set.
    ///
    /// A `virtualStoreOnly` install does no hoisting, so the patterns it
    /// records in `.modules.yaml` must be empty — that is how the next
    /// ordinary install learns hoisting still has to be done from
    /// scratch rather than reading a pattern it never applied.
    ///
    /// [`virtual_store_only`]: Self::virtual_store_only
    pub fn apply_virtual_store_only_derivation(&mut self) {
        if !self.virtual_store_only {
            return;
        }
        if self.hoist_patterns_before_virtual_store_only.is_none() {
            self.hoist_patterns_before_virtual_store_only = Some(HoistPatterns {
                hoist_pattern: self.hoist_pattern.take(),
                public_hoist_pattern: self.public_hoist_pattern.take(),
            });
        }
        self.hoist_pattern = Some(Vec::new());
        self.public_hoist_pattern = Some(Vec::new());
    }

    /// Undo [`apply_virtual_store_only_derivation`] after a command-line
    /// `--no-virtual-store-only` outranks a lower layer's
    /// `virtualStoreOnly: true`, which emptied both patterns when the
    /// config was built. pnpm merges the command line before it derives,
    /// so it never empties them in the first place.
    ///
    /// [`apply_virtual_store_only_derivation`]: Self::apply_virtual_store_only_derivation
    pub fn restore_hoist_patterns_after_virtual_store_only(&mut self) {
        if self.virtual_store_only {
            return;
        }
        if let Some(patterns) = self.hoist_patterns_before_virtual_store_only.take() {
            self.hoist_pattern = patterns.hoist_pattern;
            self.public_hoist_pattern = patterns.public_hoist_pattern;
        }
    }

    /// The lockfile file name this install reads first and writes back:
    /// the branch lockfile under `gitBranchLockfile`, `pnpm-lock.yaml`
    /// otherwise.
    ///
    /// `mergeGitBranchLockfiles` wins over the branch name — the point of
    /// that mode is to collapse the per-branch lockfiles back into the
    /// shared one.
    #[must_use]
    pub fn wanted_lockfile_name(&self) -> &str {
        match &self.git_branch_lockfile_name {
            Some(name) if !self.merge_git_branch_lockfiles => name,
            _ => Lockfile::FILE_NAME,
        }
    }

    /// [`Self::wanted_lockfile_name`] paired with the merge flag, as the
    /// lockfile loader wants them.
    #[must_use]
    pub fn wanted_lockfile_selection(&self) -> WantedLockfileSelection {
        WantedLockfileSelection {
            file_name: self.wanted_lockfile_name().to_owned(),
            merge_git_branch_lockfiles: self.merge_git_branch_lockfiles,
            branch_lockfile_candidates: self.git_branch_lockfile_candidates.clone(),
        }
    }

    /// Resolve the per-branch lockfile settings against the git branch the
    /// process is on: which `pnpm-lock.<branch>.yaml` an install under
    /// `gitBranchLockfile` uses, and whether
    /// `mergeGitBranchLockfilesBranchPattern` puts this branch in merge
    /// mode.
    ///
    /// A detached HEAD names no branch. The checked-out commit still belongs
    /// to the branches whose history includes it, so their lockfiles join
    /// the read path through [`Self::git_branch_lockfile_candidates`] — the
    /// read tries each before `pnpm-lock.yaml`. The write target stays
    /// `pnpm-lock.yaml`, the same behavior as `mergeGitBranchLockfiles`,
    /// because a branch containing HEAD need not have HEAD at its tip.
    ///
    /// The branch is read from the process's working directory, which is
    /// where pnpm reads it from too — not from the workspace root, which
    /// may sit in a different repository than the one the user is in.
    pub fn apply_git_branch_lockfile_derivation<Sys: GetCurrentDir>(&mut self) {
        // An explicit `mergeGitBranchLockfiles` — including an explicit
        // `false` — settles the question without consulting the pattern.
        let merge_is_explicit = self.explicit_settings.contains_key("mergeGitBranchLockfiles");
        let pattern_decides = !merge_is_explicit
            && !self.merge_git_branch_lockfiles_branch_pattern.is_empty();
        if !self.use_git_branch_lockfile && !pattern_decides {
            return;
        }
        let Ok(cwd) = Sys::current_dir() else { return };
        let Some(branch) = get_current_branch::<GitHost>(&cwd) else {
            self.apply_detached_head_branch_candidates(&cwd);
            return;
        };
        if pattern_decides {
            self.merge_git_branch_lockfiles =
                create_matcher(&self.merge_git_branch_lockfiles_branch_pattern).matches(&branch);
        }
        if self.use_git_branch_lockfile {
            self.git_branch_lockfile_name = Some(Lockfile::git_branch_file_name(&branch));
        }
    }

    /// Fill [`Self::git_branch_lockfile_candidates`] for a detached HEAD
    /// under plain `gitBranchLockfile`, whose branch file the loader tries
    /// before the shared one. Merge mode folds every branch lockfile in
    /// regardless of the branch, so it needs no candidates.
    fn apply_detached_head_branch_candidates(&mut self, cwd: &Path) {
        if !self.use_git_branch_lockfile || self.merge_git_branch_lockfiles {
            return;
        }
        self.git_branch_lockfile_candidates =
            detached_head_candidates(&get_branches_containing_head::<GitHost>(cwd));
    }

    /// Record the settings `settings` sets in [`Self::explicit_settings`],
    /// as loading a settings file does, so the derivations that read whether
    /// a setting was set at all see them.
    pub fn record_explicit_settings(&mut self, settings: &WorkspaceSettings) {
        collect_explicit_settings(&mut self.explicit_settings, settings);
    }

    /// Apply the legacy `shamefullyHoist` setting to the public hoist pattern.
    ///
    /// This runs after all config sources have been merged because an explicit
    /// `shamefullyHoist` value takes precedence over `publicHoistPattern`
    /// regardless of which source supplied either setting.
    pub fn apply_shamefully_hoist_derivation(&mut self) {
        match self.explicit_settings.get("shamefullyHoist").and_then(serde_json::Value::as_bool) {
            Some(true) => self.public_hoist_pattern = Some(vec!["*".to_string()]),
            Some(false) => self.public_hoist_pattern = None,
            None => {}
        }
    }

    /// Turn [`prefer_symlinked_executables`] on when the hoisted
    /// `nodeLinker` is selected and the user has not configured the
    /// setting — pnpm's `nodeLinker: hoisted` default. Runs *after* the
    /// `NODE_PATH` export in [`Config::current`], so the derived `true`
    /// symlinks bins without exporting `NODE_PATH` (the hoisted layout
    /// has no hidden store to expose), exactly like pnpm's config
    /// reader.
    ///
    /// A user-configured value — recorded in `explicit_settings` by
    /// every config layer — is never touched. Otherwise the derived
    /// value tracks the *current* linker, so re-running after a linker
    /// override also clears a `true` derived for a linker that is no
    /// longer selected (pnpm merges CLI options before its `nodeLinker`
    /// switch, so its derivation only ever sees the final linker).
    ///
    /// [`prefer_symlinked_executables`]: Self::prefer_symlinked_executables
    pub fn apply_prefer_symlinked_executables_derivation(&mut self) {
        if self.explicit_settings.contains_key("preferSymlinkedExecutables") {
            return;
        }
        self.prefer_symlinked_executables =
            (self.node_linker == NodeLinker::Hoisted).then_some(true);
    }

    /// Restore the smart default store after a higher-precedence config
    /// source explicitly clears `storeDir`.
    pub fn reset_store_dir_to_default<Sys>(&mut self, start_dir: &Path)
    where
        Sys: EnvVar + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        self.store_dir = default_store_dir::<Sys>();
        self.resolve_default_store_dir::<Sys>(start_dir);
        self.explicit_settings.remove("storeDir");
        let virtual_store_dir_explicit = self.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            self.explicit_settings.contains_key("globalVirtualStoreDir");
        self.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }

    /// Resolve the default store location relative to an explicit pnpm home
    /// directory instead of the ambient one — the programmatic counterpart
    /// of the `pnpmHomeDir` input of pnpm's `getStorePath`. The store lands
    /// at `<pnpm_home_dir>/store/<version>` when `start_dir` can hardlink
    /// into that volume, with the same mount-point fallback as the ambient
    /// default. Callers apply it only when no config source set `storeDir`.
    pub fn resolve_store_dir_from_home<Sys>(&mut self, pnpm_home_dir: &Path, start_dir: &Path)
    where
        Sys: GetHomeDir + LinkProbe,
    {
        self.store_dir = StoreDir::new(pnpm_home_dir.join("store"));
        self.resolve_default_store_dir::<Sys>(start_dir);
    }

    /// Place a store the load left unplaced
    /// ([`store_dir_placement_skipped`](Self::store_dir_placement_skipped))
    /// where a store consumer's load from `start_dir` would have placed it.
    pub fn place_skipped_store_dir<Sys>(&mut self, start_dir: &Path)
    where
        Sys: GetHomeDir + LinkProbe,
    {
        if !std::mem::take(&mut self.store_dir_placement_skipped) {
            return;
        }
        self.skip_store_dir_resolution = false;
        self.resolve_default_store_dir::<Sys>(start_dir);
        let virtual_store_dir_explicit = self.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            self.explicit_settings.contains_key("globalVirtualStoreDir");
        self.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }

    pub(super) fn resolve_default_store_dir<Sys: GetHomeDir + LinkProbe>(
        &mut self,
        start_dir: &Path,
    ) {
        if self.skip_store_dir_resolution {
            self.store_dir_placement_skipped = true;
            return;
        }
        let Some(home_dir) = Sys::home_dir() else {
            return;
        };
        // `store_dir.root()` includes the layout version, so its parent is
        // the unversioned store and the next parent is pnpm's home directory.
        // The linkability probe only cares about that directory's volume;
        // fall back to the user's home when either parent is unavailable.
        let store_root_versioned = self.store_dir.root().to_path_buf();
        let store_root = store_root_versioned
            .parent()
            .unwrap_or(&home_dir)
            .to_path_buf();
        let pnpm_home_dir = store_root
            .parent()
            .unwrap_or(&home_dir)
            .to_path_buf();
        let resolved = StoreDir::from(store_path::resolve_store_dir::<Sys>(
            store_root.clone(),
            &pnpm_home_dir,
            start_dir,
        ));
        let home_store_dir = StoreDir::from(store_root);
        self.store_relocation = (resolved != home_store_dir).then(|| {
            Box::new(StoreRelocation { home_store_dir, store_dir: resolved.clone() })
        });
        self.store_dir = resolved;
    }

    /// The warning to print when the default store was moved off the pnpm
    /// home directory while a store already exists there, so packages
    /// already in it are downloaded again. [`None`] when the store in use
    /// is not the relocated one (an explicit `storeDir` replaced it) or the
    /// home store does not exist.
    pub fn bypassed_home_store_warning(&self) -> Option<String> {
        let relocation = self.store_relocation.as_ref()?;
        (relocation.store_dir == self.store_dir && relocation.home_store_dir.root().is_dir()).then(
            || relocation.warning(),
        )
    }

    /// The directory packages are linked into: the global virtual store
    /// when [`Self::enable_global_virtual_store`] is on, otherwise
    /// [`Self::install_state_dir`]. This is the `virtualStoreDir` pnpm
    /// records in `.modules.yaml` and emits in the `pnpm:context` event, so
    /// reading the install state directory instead makes the next install
    /// fail with `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE_DIR`.
    pub fn virtual_store_dir(&self) -> &Path {
        if self.enable_global_virtual_store {
            &self.global_virtual_store_dir
        } else {
            &self.install_state_dir
        }
    }
}

/// `raw`, the configured `modulesDir`, with a leading `./` dropped, when
/// it names a directory inside the project and `modules_dir` is still
/// the path resolved from it. `None` otherwise, which leaves callers
/// with the basename of `modules_dir`.
pub(crate) fn project_relative_modules_dir<'a>(
    raw: &'a str,
    modules_dir: &Path,
) -> Option<&'a Path> {
    let raw = Path::new(raw);
    let relative = raw.strip_prefix(".").unwrap_or(raw);
    let mut components = relative.components().peekable();
    (components.peek().is_some()
        && components.all(|component| matches!(component, std::path::Component::Normal(_)))
        && modules_dir.ends_with(relative))
    .then_some(relative)
}

/// The lockfile names of `branches`, without the ones longer than a
/// filesystem allows: such a file cannot be on disk, and probing it fails
/// with `ENAMETOOLONG` instead of reporting it absent.
pub(crate) fn detached_head_candidates(branches: &[String]) -> Vec<String> {
    const MAX_FILE_NAME_LENGTH: usize = 255;
    branches
        .iter()
        .map(|branch| Lockfile::git_branch_file_name(branch))
        .filter(|file_name| file_name.len() <= MAX_FILE_NAME_LENGTH)
        .collect()
}
