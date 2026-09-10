use super::{
    Config, EnvVar, GetCurrentDir, GetHomeDir, GitHost, HashMap, HoistPatterns, LinkProbe,
    Lockfile, NodeLinker, Path, StoreDir, WantedLockfileSelection, WorkspaceSettings,
    collect_explicit_settings, create_matcher, default_store_dir, esm_node_path_loader,
    get_current_branch, store_path,
};

impl Config {
    /// Resolve relative patch file paths in
    /// [`Config::patched_dependencies`] against
    /// [`Config::workspace_dir`], compute SHA-256 hashes, and bucket
    /// the entries into a [`PatchGroupRecord`](super::PatchGroupRecord).
    ///
    /// Resolves each configured patch path against the workspace dir,
    /// then hashes the files.
    ///
    /// Returns `Ok(None)` when either field is unset (no yaml
    /// found or no `patchedDependencies` key). Returns `Err(_)`
    /// when any patch file can't be hashed or any key has an
    /// invalid semver range.
    ///
    /// IO-heavy; call once per install rather than at every site
    /// that needs the resolved record.
    /// Derive [`Self::global_virtual_store_dir`] from
    /// `enable_global_virtual_store` + the existing `store_dir` /
    /// `virtual_store_dir` fields.
    ///
    /// Pacquet diverges from pnpm on *which* field carries the GVS path:
    ///
    /// - **pnpm**: mutates `virtualStoreDir` in place when GVS is
    ///   on and the user hasn't pinned it, so every consumer that
    ///   reads `virtualStoreDir` ends up looking at `<storeDir>/links`.
    /// - **Pacquet**: keeps `virtual_store_dir` at its project-local
    ///   value (`<cwd>/node_modules/.pnpm` by default, or the user's
    ///   yaml-pinned path) and writes the GVS path into the separate
    ///   `global_virtual_store_dir` field. The install layer picks the
    ///   right field through [`crate::Config::enable_global_virtual_store`]
    ///   (or, in practice, through `pnpm_package_manager::VirtualStoreLayout`).
    ///
    /// The reason: pacquet still has a non-frozen
    /// `InstallWithFreshLockfile` path that pnpm doesn't have.
    /// Mutating `virtual_store_dir` would redirect that path to
    /// `<storeDir>/links` too — but the issue (pnpm/pacquet#432)
    /// scopes GVS to frozen-lockfile installs. Splitting the field
    /// keeps the fresh-lockfile path on the project-local layout
    /// while the frozen-lockfile path consumes the GVS-derived value.
    ///
    /// `virtual_store_dir_explicit` carries the "did the user set
    /// `virtualStoreDir` in yaml" signal `SmartDefault` cannot express
    /// on its own. When `true` *and* GVS is on, `global_virtual_store_dir`
    /// mirrors `virtual_store_dir` (the user picked the GVS root via the
    /// shared key). `global_virtual_store_dir_explicit` is the analogous
    /// signal for the dedicated `globalVirtualStoreDir` yaml key — when
    /// set, that value wins and the derivation leaves
    /// `global_virtual_store_dir` alone. Otherwise the field falls back
    /// to `<store_dir>/links`, an unconditional
    /// `globalVirtualStoreDir = storeDir/links` assignment for the unset
    /// case.
    pub fn apply_global_virtual_store_derivation(
        &mut self,
        virtual_store_dir_explicit: bool,
        global_virtual_store_dir_explicit: bool,
    ) {
        if global_virtual_store_dir_explicit {
            // User pinned the dedicated GVS key in yaml — honor it.
            return;
        }
        self.global_virtual_store_dir =
            if self.enable_global_virtual_store && virtual_store_dir_explicit {
                self.virtual_store_dir.clone()
            } else {
                self.store_dir.links()
            };
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
        self.lockfile_dir.as_deref().unwrap_or_else(|| {
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
        self.lockfile_dir.as_deref().or(self.workspace_dir.as_deref()).unwrap_or(dir)
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
    /// Global-virtual-store installs keep their store-anchored
    /// `virtual_store_dir`.
    ///
    /// [`explicit_settings`]: Self::explicit_settings
    pub fn anchor_lockfile_paths(&mut self, dir: &Path) {
        self.modules_dir =
            match self.explicit_settings.get("modulesDir").and_then(serde_json::Value::as_str) {
                Some(raw) => dir.join(raw),
                None => dir.join("node_modules"),
            };
        if !self.enable_global_virtual_store {
            self.virtual_store_dir = match self
                .explicit_settings
                .get("virtualStoreDir")
                .and_then(serde_json::Value::as_str)
            {
                Some(raw) => dir.join(raw),
                None => self.modules_dir.join(".pnpm"),
            };
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
        let Some(project_config) =
            project_name.and_then(|name| self.package_configs.as_ref()?.get(name)).cloned()
        else {
            return;
        };
        project_config.apply_to(self, project_dir);
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
        }
    }

    /// Resolve the per-branch lockfile settings against the git branch the
    /// process is on: which `pnpm-lock.<branch>.yaml` an install under
    /// `gitBranchLockfile` uses, and whether
    /// `mergeGitBranchLockfilesBranchPattern` puts this branch in merge
    /// mode.
    ///
    /// The branch is read from the process's working directory, which is
    /// where pnpm reads it from too — not from the workspace root, which
    /// may sit in a different repository than the one the user is in.
    pub fn apply_git_branch_lockfile_derivation<Sys: GetCurrentDir>(&mut self) {
        // An explicit `mergeGitBranchLockfiles` — including an explicit
        // `false` — settles the question without consulting the pattern.
        let merge_is_explicit = self.explicit_settings.contains_key("mergeGitBranchLockfiles");
        let pattern_decides =
            !merge_is_explicit && !self.merge_git_branch_lockfiles_branch_pattern.is_empty();
        if !self.use_git_branch_lockfile && !pattern_decides {
            return;
        }
        let Ok(cwd) = Sys::current_dir() else { return };
        let Some(branch) = get_current_branch::<GitHost>(&cwd) else { return };
        if pattern_decides {
            self.merge_git_branch_lockfiles =
                create_matcher(&self.merge_git_branch_lockfiles_branch_pattern).matches(&branch);
        }
        if self.use_git_branch_lockfile {
            self.git_branch_lockfile_name = Some(Lockfile::git_branch_file_name(&branch));
        }
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
    /// reader. Also re-applied by the CLI's `--config.node-linker`
    /// override, which lands after [`Config::current`] has run.
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

    pub(super) fn resolve_default_store_dir<Sys: GetHomeDir + LinkProbe>(
        &mut self,
        start_dir: &Path,
    ) {
        let Some(home_dir) = Sys::home_dir() else {
            return;
        };
        // `store_dir.root()` includes the layout version, so its parent is
        // the unversioned store and the next parent is pnpm's home directory.
        // The linkability probe only cares about that directory's volume;
        // fall back to the user's home when either parent is unavailable.
        let store_root_versioned = self.store_dir.root().to_path_buf();
        let store_root = store_root_versioned.parent().unwrap_or(&home_dir).to_path_buf();
        let pnpm_home_dir = store_root.parent().unwrap_or(&home_dir).to_path_buf();
        let resolved = store_path::resolve_store_dir::<Sys>(store_root, &pnpm_home_dir, start_dir);
        self.store_dir = StoreDir::from(resolved);
    }

    /// Return the `virtualStoreDir` value pnpm exposes externally — the
    /// path written into `.modules.yaml` and emitted in the `pnpm:context`
    /// NDJSON event.
    ///
    /// pnpm mutates `virtualStoreDir` in place when
    /// `enableGlobalVirtualStore` is on and the user hasn't pinned
    /// `virtualStoreDir`, so every consumer that reads `ctx.virtualStoreDir`
    /// — including the modules-manifest writer and the `pnpm:context`
    /// debug log — sees the GVS-derived path.
    ///
    /// Pacquet deliberately keeps [`Self::virtual_store_dir`] at its
    /// project-local value (see [`Self::apply_global_virtual_store_derivation`]
    /// for the why), so consumers that need the externally-observable
    /// value must route through this helper instead of reading the field
    /// directly. Otherwise the `.modules.yaml` round-trip mismatches
    /// pnpm's, and the next `pnpm install` trips
    /// `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE_DIR` → forces a
    /// "modules directories will be reinstalled from scratch" prompt
    /// on every install.
    pub fn effective_virtual_store_dir(&self) -> &Path {
        if self.enable_global_virtual_store {
            &self.global_virtual_store_dir
        } else {
            &self.virtual_store_dir
        }
    }
}
