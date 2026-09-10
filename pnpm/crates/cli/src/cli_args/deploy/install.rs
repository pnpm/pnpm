use super::{
    Arc, Config, Context, DeployArgs, DeployInstallMode, Install, LazyLockfile, Lockfile,
    NodeLinker, NodeLinkerArg, Path, PreferredVersions, Reporter, State, WantedLockfileSelection,
    deployed_workspace_projects, get_preferred_versions_from_lockfile_and_manifests,
    resolve_bool_override, warn,
};

/// The lockfile a shared deploy generates records no
/// `pnpmfileChecksum`, so the pnpmfile behind these hooks must not
/// claim one either. The legacy path resolves the deployed project
/// from scratch and writes its own lockfile, which records the
/// checksum as any other install does.
fn deploy_pnpmfile_hooks(
    source_hooks: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    legacy: bool,
) -> Option<Arc<dyn pnpm_hooks::PnpmfileHooks>> {
    source_hooks.map(|hooks| -> Arc<dyn pnpm_hooks::PnpmfileHooks> {
        if legacy { hooks } else { Arc::new(pnpm_hooks::ChecksumFreeHooks::from(hooks)) }
    })
}

/// A shared deploy installs the deployed project as a workspace of its
/// own, so none of the source workspace's graph-level settings apply to
/// it. The legacy path resolves from scratch and keeps them.
fn apply_shared_deploy_config(config: &mut Config, deploy_dir: &Path, mode: DeployInstallMode) {
    let DeployInstallMode::Shared { workspace_config } = mode else {
        return;
    };
    config.workspace_dir = deploy_dir.to_path_buf().into();
    config.inject_workspace_packages = false;
    config.overrides = None;
    config.package_extensions = None;
    config.config_dependencies = None;
    config.patched_dependencies = workspace_config.patched_dependencies;
    config.allow_builds = workspace_config.allow_builds;
}

/// The lockfile a legacy deploy install reads and writes.
///
/// The deployed project is not one of the source workspace's importers —
/// the deploy hook rewrites the copied manifest — so its resolution must
/// not be seeded from the workspace lockfile. Plain `pnpm-lock.yaml`
/// whatever the branch settings say, for the same reason: they describe
/// that workspace's resolution, and pnpm's deploy reads and writes the
/// deployed lockfile under the plain name too.
fn deployed_lockfile(state: &State, deploy_dir: &Path, frozen_lockfile: bool) -> LazyLockfile {
    if state.config.lockfile || frozen_lockfile {
        LazyLockfile::deferred(deploy_dir.to_path_buf(), WantedLockfileSelection::default())
    } else {
        LazyLockfile::disabled()
    }
}

/// The pnpmfile the deploy install runs, resolved the way an install of
/// the selected project resolves its own: from the source workspace,
/// whose hooks shape the dependency graph the deploy writes out.
pub(super) fn source_pnpmfile_hooks(
    config: &Config,
    project_dir: &Path,
) -> miette::Result<Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>> {
    if config.ignore_pnpmfile {
        return Ok(None);
    }
    pnpm_hooks::finder::load_pnpmfiles(
        config.lockfile_dir_for(project_dir),
        pnpm_package_manager::pnpmfile_selection(config),
    )
    .map_err(|error| miette::miette!(code = "ERR_PNPM_PNPMFILE_NOT_FOUND", "{error}"))
}

/// Loads source lockfile pins as preferred versions for legacy deploy.
/// The source install's branch-lockfile selection is preserved, while a
/// missing or malformed source lockfile falls back to fresh resolution.
pub(super) fn legacy_deploy_preferred_versions<ReporterT: Reporter>(
    config: &Config,
    source_lockfile_dir: &Path,
) -> Option<PreferredVersions> {
    if !config.lockfile {
        return None;
    }
    match Lockfile::load_wanted(source_lockfile_dir, &config.wanted_lockfile_selection()) {
        Ok(Some(lockfile)) => Some(get_preferred_versions_from_lockfile_and_manifests(
            lockfile.snapshots.as_ref(),
            &[],
        )),
        Ok(None) => None,
        Err(error) => {
            warn::<ReporterT>(
                source_lockfile_dir,
                format!("Ignoring broken lockfile at {}: {error}", source_lockfile_dir.display()),
            );
            None
        }
    }
}

pub(super) fn create_deploy_install_config(
    base_config: &Config,
    deploy_dir: &Path,
    node_linker: NodeLinker,
) -> Config {
    let mut deploy_config = base_config.clone();
    deploy_config.modules_dir = deploy_dir.join("node_modules");
    deploy_config.virtual_store_dir = deploy_dir.join("node_modules").join(".pnpm");
    // The deploy directory owns the lockfile this install runs against —
    // the generated one for a shared deploy, its own resolution for the
    // legacy path. A `lockfileDir` pinning the *source* workspace's
    // lockfile must not redirect either.
    deploy_config.lockfile_dir = None;
    deploy_config.global_virtual_store_dir = deploy_config.virtual_store_dir.clone();
    deploy_config.enable_global_virtual_store = false;
    deploy_config.pnpr_server = None;
    deploy_config.optimistic_repeat_install = false;
    deploy_config.dedupe_peer_dependents = false;
    deploy_config.dedupe_injected_deps = false;
    deploy_config.node_linker = node_linker;
    deploy_config
}

impl DeployArgs {
    pub(super) async fn run_install_in_deploy_dir<ReporterT: Reporter + 'static>(
        &self,
        base_config: &Config,
        deploy_dir: &Path,
        mode: DeployInstallMode,
        frozen_lockfile: bool,
        source_hooks: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
        preferred_versions_override: Option<PreferredVersions>,
    ) -> miette::Result<()> {
        let legacy = matches!(&mode, DeployInstallMode::Legacy);
        let config = self.deploy_install_config(
            base_config,
            deploy_dir,
            mode,
            frozen_lockfile,
            source_hooks.is_none(),
        );
        let pnpmfile_hook = deploy_pnpmfile_hooks(source_hooks, legacy);
        let mut state = State::init(deploy_dir.join("package.json"), config, frozen_lockfile)
            .wrap_err("initialize the deploy install state")?;
        if legacy {
            state.lockfile = deployed_lockfile(&state, deploy_dir, frozen_lockfile);
        }
        self.install_deployed_state::<ReporterT>(
            &state,
            deploy_dir,
            legacy,
            frozen_lockfile,
            pnpmfile_hook,
            preferred_versions_override,
        )
        .await
    }

    async fn install_deployed_state<ReporterT: Reporter + 'static>(
        &self,
        state: &State,
        deploy_dir: &Path,
        legacy: bool,
        frozen_lockfile: bool,
        pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
        preferred_versions_override: Option<PreferredVersions>,
    ) -> miette::Result<()> {
        let config = state.config;
        let workspace_projects_override = deployed_workspace_projects(state, deploy_dir, legacy);

        let supported_architectures = self
            .install_args
            .supported_architectures
            .apply_to(config.supported_architectures.clone());
        let trust_lockfile = resolve_bool_override(
            self.install_args.trust_lockfile,
            self.install_args.no_trust_lockfile,
            config.trust_lockfile,
        );
        let lockfile_path = config.lockfile.then(|| deploy_dir.join(Lockfile::FILE_NAME));
        let dependency_groups = self
            .install_args
            .dependency_options
            .dependency_groups(config.optional)
            .collect::<Vec<_>>();

        let install = Install {
            lockfile_path: lockfile_path.as_deref(),
            frozen_lockfile,
            prefer_frozen_lockfile: frozen_lockfile.then_some(true).or(Some(false)),
            skip_runtimes: config.skip_runtimes || self.install_args.no_runtime,
            trust_lockfile,
            supported_architectures,
            preferred_versions_override,
            disable_optimistic_repeat_install: true,
            pnpmfile_hook_override: pnpmfile_hook,
            workspace_projects_override,
            ..state.install(dependency_groups)
        };
        if legacy {
            install.run_legacy_deploy::<ReporterT>().await
        } else {
            install.run::<ReporterT>().await
        }
        .wrap_err("installing deployed dependencies")
    }

    /// The deploy directory's own install config, with the install flags
    /// the deploy forwards.
    fn deploy_install_config(
        &self,
        base_config: &Config,
        deploy_dir: &Path,
        mode: DeployInstallMode,
        frozen_lockfile: bool,
        ignore_pnpmfile: bool,
    ) -> &'static Config {
        let node_linker = self
            .install_args
            .node_linker
            .map_or(base_config.node_linker, NodeLinkerArg::into_config);
        let mut deploy_config = create_deploy_install_config(base_config, deploy_dir, node_linker);
        deploy_config.prefer_frozen_lockfile = frozen_lockfile;
        // pnpm's deploy forwards `--force` into the install, where it
        // bypasses the installability check so optional dependencies of
        // every platform are materialized (see `Config::force`).
        deploy_config.force = self.install_args.force;
        // `source_hooks` is the whole of the pnpmfile this install runs.
        // With none to run there is nothing left to discover either: the
        // install must not fall back to looking next to the deployed
        // manifest, where `copy_project` may have left the deployed
        // project's own pnpmfile.
        deploy_config.ignore_pnpmfile = ignore_pnpmfile;
        apply_shared_deploy_config(&mut deploy_config, deploy_dir, mode);
        Config::leak(deploy_config)
    }
}
