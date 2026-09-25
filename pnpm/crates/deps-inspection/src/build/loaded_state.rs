use super::{
    BTreeMap, Context, DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, HashMap, HashSet, Host,
    IntoDiagnostic, Lockfile, Modules, Path, PathBuf, PkgInfoEnv, RegistryOptions,
    detect_dep_types, lexical_normalize, read_modules_manifest,
};

/// The lockfiles and modules-manifest state one tree build runs
/// against. Owns the loaded lockfiles; [`LoadedState::env`] borrows them.
pub struct LoadedState {
    pub modules_dir: PathBuf,
    pub modules: Option<Modules>,
    pub current_lockfile: Option<Lockfile>,
    pub wanted_lockfile: Option<Lockfile>,
    pub check_wanted_lockfile_only: bool,
}

impl LoadedState {
    pub fn load(
        lockfile_dir: &Path,
        modules_dir_opt: Option<&Path>,
        check_wanted_lockfile_only: bool,
    ) -> miette::Result<LoadedState> {
        let modules_dir_raw = match modules_dir_opt {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            Some(dir) => lockfile_dir.join(dir),
            None => lockfile_dir.join("node_modules"),
        };
        let modules_dir = pnpm_fs::realpath_missing(&modules_dir_raw)
            .unwrap_or_else(|_| lexical_normalize(&modules_dir_raw));
        let modules = read_modules_manifest::<Host>(&modules_dir)
            .into_diagnostic()
            .wrap_err("read the modules manifest")?;
        let current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&modules_dir.join(".pnpm"))
                .into_diagnostic()
                .wrap_err("load the current lockfile")?;
        let wanted_lockfile = Lockfile::load_wanted_from_dir(lockfile_dir)
            .into_diagnostic()
            .wrap_err("load the wanted lockfile")?;
        Ok(LoadedState {
            modules_dir,
            modules,
            current_lockfile,
            wanted_lockfile,
            check_wanted_lockfile_only,
        })
    }

    /// The lockfile the tree is built from: the wanted lockfile under
    /// `--lockfile-only`, otherwise the current lockfile with the
    /// wanted one as fallback.
    #[must_use]
    pub fn lockfile_to_use(&self) -> Option<&Lockfile> {
        if self.check_wanted_lockfile_only {
            self.wanted_lockfile.as_ref()
        } else {
            self.current_lockfile.as_ref().or(self.wanted_lockfile.as_ref())
        }
    }

    /// [`Self::env`] with every setting read from `config`.
    #[must_use]
    pub fn env_for_config<'a>(
        &'a self,
        lockfile_dir: &Path,
        config: &pnpm_config::Config,
    ) -> Option<PkgInfoEnv<'a>> {
        self.env(
            lockfile_dir,
            config.virtual_store_dir_max_length as usize,
            &config.resolved_registries(),
            config.registry_options_by_url.clone(),
            config.peer_edge_options(),
        )
    }

    #[must_use]
    pub fn env<'a>(
        &'a self,
        lockfile_dir: &Path,
        virtual_store_dir_max_length: usize,
        registries_by_scope: &BTreeMap<String, String>,
        registry_options_by_url: BTreeMap<String, RegistryOptions>,
        peer_edges: pnpm_lockfile::PeerEdgeOptions,
    ) -> Option<PkgInfoEnv<'a>> {
        let lockfile = self.lockfile_to_use()?;
        let registries: HashMap<String, String> = registries_by_scope
            .iter()
            .map(|(scope, registry)| (scope.clone(), registry.clone()))
            .collect();
        Some(PkgInfoEnv {
            registries,
            registry_options_by_url,
            skipped: self.modules
                .as_ref()
                .map(|modules| {
                    modules.skipped
                        .iter()
                        .cloned()
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default(),
            current_lockfile: lockfile,
            wanted_lockfile: self.wanted_lockfile.as_ref(),
            dep_types: detect_dep_types(
                lockfile,
                &pnpm_lockfile::PeerSatisfactionEdges::of_lockfile(lockfile, peer_edges),
            ),
            layout: self.layout(lockfile_dir, virtual_store_dir_max_length),
        })
    }
    fn layout(
        &self,
        lockfile_dir: &Path,
        virtual_store_dir_max_length: usize,
    ) -> crate::pkg_info::InspectionLayout {
        let virtual_store_dir = match &self.modules {
            Some(modules) if !modules.virtual_store_dir.is_empty() => {
                let dir = PathBuf::from(&modules.virtual_store_dir);
                if dir.is_absolute() { dir } else { self.modules_dir.join(dir) }
            }
            _ => self.modules_dir.join(".pnpm"),
        };
        crate::pkg_info::InspectionLayout {
            lockfile_dir: lockfile_dir.to_path_buf(),
            modules_dir: self.modules_dir.clone(),
            virtual_store_dir,
            virtual_store_dir_max_length: self.modules
                .as_ref()
                .map_or(virtual_store_dir_max_length, |modules| {
                    usize::try_from(modules.virtual_store_dir_max_length)
                        .unwrap_or(DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH as usize)
                }),
            store_dir: self.modules
                .as_ref()
                .map(|modules| PathBuf::from(&modules.store_dir))
                .filter(|dir| !dir.as_os_str().is_empty()),
        }
    }
}
