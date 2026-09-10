use super::{
    Config, EnvVar, EnvVarOs, ExplicitPaths, GLOBAL_LAYOUT_VERSION, GetCurrentDir, GetHomeDir,
    LinkProbe, LoadWorkspaceYamlError, NodeLinker, NpmrcAuth, default_pnpm_home_dir,
    esm_node_path_loader,
};

impl Config {
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
        let global_dir_root = self
            .global_dir
            .clone()
            .or_else(|| pnpm_home_dir.as_ref().map(|home| home.join("global")));
        self.global_pkg_dir = global_dir_root.map(|root| root.join(GLOBAL_LAYOUT_VERSION));
        self.global_bin = self
            .global_bin_dir
            .clone()
            .or_else(|| pnpm_home_dir.as_ref().map(|home| home.join("bin")));

        // Inside a workspace, scripts and `pnpm exec` also get the
        // workspace root's `node_modules/.bin` on PATH — pnpm's
        // `extraBinPaths = [join(workspaceDir, 'node_modules', '.bin')]`.
        self.extra_bin_paths = self
            .workspace_dir
            .as_deref()
            .map(|dir| vec![dir.join("node_modules").join(".bin")])
            .unwrap_or_default();

        // With `preferSymlinkedExecutables`, `.bin` entries are plain
        // symlinks with no shim to carry a `NODE_PATH` block, so the
        // resolution help moves to the environment: expose the virtual
        // store's hidden `node_modules` to every spawned child process.
        // `virtual_store_dir` is already anchored at the workspace root
        // by the re-anchor above — pnpm builds this from
        // `lockfileDir ?? dir` to the same effect
        // (pnpm/pnpm#13912). Unix only, like pnpm; and only an explicit
        // `true` fires — the hoisted-linker derivation below runs after
        // this block, mirroring pnpm's config-reader ordering.
        if cfg!(unix) && self.prefer_symlinked_executables == Some(true) {
            let hidden_modules_dir =
                pnpm_fs::lexical_normalize(&self.virtual_store_dir.join("node_modules"));
            self.extra_env
                .insert("NODE_PATH".to_string(), hidden_modules_dir.display().to_string());
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
    pub(super) fn apply_global_virtual_store_node_path<Sys>(&mut self)
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
        let mut node_paths: Vec<String> = self
            .extra_env
            .get("NODE_PATH")
            .map(|value| value.split(path_delimiter).map(str::to_string).collect())
            .unwrap_or_default();
        for dir in [self.virtual_store_dir.join("node_modules"), self.modules_dir.clone()] {
            // `virtual_store_dir` is built by joining a multi-segment
            // literal, which keeps `/` separators on Windows; normalize
            // so NODE_PATH carries native separators like the shims do.
            let dir = pnpm_fs::lexical_normalize(&dir).display().to_string();
            if !node_paths.contains(&dir) {
                node_paths.push(dir);
            }
        }
        self.extra_env
            .insert("NODE_PATH".to_string(), node_paths.join(&path_delimiter.to_string()));
        self.extra_env.insert(
            "NODE_OPTIONS".to_string(),
            esm_node_path_loader::add_esm_node_path_loader_option(
                Sys::var("NODE_OPTIONS").as_deref(),
            ),
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
        // A pinned `lockfileDir` moves the root `node_modules` and the
        // virtual store with it. Applied after every source has had its
        // say so the anchor uses the final value, and before the
        // global-virtual-store derivation, which may re-point
        // `virtual_store_dir` at the store.
        if let Some(lockfile_dir) = self.lockfile_dir.clone() {
            self.anchor_lockfile_paths(&lockfile_dir);
        }

        // Build the per-URI auth-header lookup. Credentials were already
        // pinned to their source file's registry by `rescope_unscoped`,
        // so this is independent of the final `config.registry` (which
        // yaml may have overridden) — the security boundary holds even
        // when the workspace points the default registry elsewhere.
        std::mem::take(npmrc_auth).build_auth_headers(self)?;

        // Re-resolve `store_dir` against the project's volume when no
        // explicit source (global config.yaml, pnpm-workspace.yaml,
        // `PNPM_CONFIG_STORE_DIR`) set it. The SmartDefault picks
        // `<pnpm_home>/store` unconditionally; the store-path resolution
        // probes whether `pkg_root` can hardlink into the home volume
        // and falls back to `<mountpoint>/.pnpm-store` when it can't,
        // so a workspace on a separate (case-sensitive) volume gets a
        // store on that same volume rather than the home volume.
        // Without this, typescript-eslint's case-folded path cache
        // diverges from TypeScript's case-sensitive program when the
        // workspace is case-sensitive and the home is not.
        if !explicit.store_dir {
            self.resolve_default_store_dir::<Sys>(start_dir);
        }

        // Derive `global_virtual_store_dir` last so it sees the final
        // `store_dir` / `virtual_store_dir` after yaml has been
        // applied. An explicit `globalVirtualStoreDir` in yaml wins
        // over the derivation; otherwise the field falls back to the
        // user's pinned `virtualStoreDir` (under GVS-on) or to
        // `<store_dir>/links`. See
        // [`Self::apply_global_virtual_store_derivation`].
        self.apply_global_virtual_store_derivation(
            explicit.virtual_store_dir,
            explicit.global_virtual_store_dir,
        );

        self.apply_git_branch_lockfile_derivation::<Sys>();
        self.apply_shamefully_hoist_derivation();
        self.apply_virtual_store_only_derivation();
        Ok(())
    }
}
