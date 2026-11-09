use super::{
    Arc, Config, Context, DeployArgs, DeployInstallMode, LazyLockfile, Lockfile, NodeLinker,
    NodeLinkerArg, Path, PathBuf, PreferredVersions, Reporter, State, WantedLockfileSelection,
    approve_legacy_workspace_project_builds, deployed_workspace_projects,
    legacy_source::LegacyDeploySource, resolve_bool_override,
};

/// The lockfile a shared deploy generates records no
/// `pnpmfileChecksum`, so the pnpmfile behind these hooks must not
/// claim one either. A legacy deploy preserves the source checksum for
/// the resolver's hook freshness check.
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
/// it. The legacy path keeps them.
fn apply_shared_deploy_config(config: &mut Config, deploy_dir: &Path, mode: DeployInstallMode) {
    let DeployInstallMode::Shared { workspace_config } = mode else {
        return;
    };
    workspace_config.apply_to_config(config, deploy_dir);
}

/// The deploy directory's lockfile when no source lockfile can seed resolution.
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

/// A global virtual store or an absolute `virtualStoreDir` is shared with
/// the source workspace, and the self-contained deploy must not write into it.
pub(super) fn configured_virtual_store_dir(config: &Config) -> Option<&str> {
    config.explicit_settings
        .get("virtualStoreDir")
        .and_then(serde_json::Value::as_str)
        .filter(|raw| !config.enable_global_virtual_store && !Path::new(raw).is_absolute())
}

fn deploy_virtual_store_dir(base_config: &Config, deploy_dir: &Path) -> PathBuf {
    match configured_virtual_store_dir(base_config) {
        Some(raw) => pnpm_fs::lexical_normalize(&deploy_dir.join(raw)),
        None => deploy_dir.join("node_modules").join(".pnpm"),
    }
}

pub(super) fn create_deploy_install_config(
    base_config: &Config,
    deploy_dir: &Path,
    node_linker: NodeLinker,
) -> Config {
    let mut deploy_config = base_config.clone();
    deploy_config.modules_dir = deploy_dir.join("node_modules");
    deploy_config.install_state_dir = deploy_virtual_store_dir(base_config, deploy_dir);
    // The deploy directory owns the lockfile this install runs against:
    // the generated one for a shared deploy, its own resolution for the
    // legacy path. Pinning it keeps the source workspace, found above the
    // deploy directory or set by `lockfileDir`, from becoming the root its
    // importers are placed against.
    deploy_config.lockfile_dir = Some(deploy_dir.to_path_buf());
    deploy_config.global_virtual_store_dir = deploy_config.install_state_dir.clone();
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
        legacy_source: Option<LegacyDeploySource>,
    ) -> miette::Result<()> {
        let legacy = matches!(&mode, DeployInstallMode::Legacy { .. });
        let config = self.deploy_install_config(
            base_config,
            deploy_dir,
            mode,
            frozen_lockfile,
            source_hooks.is_none(),
        )?;
        let pnpmfile_hook = deploy_pnpmfile_hooks(source_hooks, legacy);
        let mut state = State::init(deploy_dir.join("package.json"), config, frozen_lockfile)
            .wrap_err("initialize the deploy install state")?;
        if legacy {
            state.lockfile = deployed_lockfile(&state, deploy_dir, frozen_lockfile);
        }
        if let Some(source) = &legacy_source
            && !self.trust_lockfile(config)
        {
            source.verify::<ReporterT>(&state).await?;
        }
        let preferred_versions = legacy_source.as_ref().map(LegacyDeploySource::preferred_versions);
        if let Some(source) = legacy_source {
            state.lockfile = LazyLockfile::preloaded(Some(source.lockfile));
        }
        self.install_deployed_state::<ReporterT>(
            &state,
            deploy_dir,
            legacy,
            frozen_lockfile,
            pnpmfile_hook,
            preferred_versions,
        )
        .await
    }

    fn trust_lockfile(&self, config: &Config) -> bool {
        resolve_bool_override(
            self.install_args.lockfile_updates.trust_lockfile,
            self.install_args.lockfile_updates.no_trust_lockfile,
            config.trust_lockfile,
        )
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

        let supported_architectures = self.install_args.supported_architectures.apply_to(
            config.supported_architectures.clone(),
        );
        let trust_lockfile = self.trust_lockfile(config);
        let lockfile_path = config.lockfile.then(|| deploy_dir.join(Lockfile::FILE_NAME));
        let dependency_groups = self.install_args.dependency_options
            .dependency_groups(config.optional)
            .collect::<Vec<_>>();

        let install = {
            let mut base_install = state.install(dependency_groups);
            base_install.execution.mutation = pnpm_package_manager::ProjectMutation::Deploy;
            base_install.lockfile_policy.frozen = frozen_lockfile;
            base_install.lockfile_policy.prefer_frozen = frozen_lockfile
                .then_some(true)
                .or(Some(false));
            base_install.lockfile_policy.trust = trust_lockfile;
            base_install.lockfile_policy.disable_optimistic_repeat = true;
            base_install.execution.skip_runtimes =
                config.skip_runtimes || self.install_args.materialization.no_runtime;
            base_install.resolution.preferred_versions_override =
                preferred_versions_override.map(Into::into);
            base_install.context.lockfile_path = lockfile_path.as_deref();
            base_install.projects.supported_architectures = supported_architectures;
            base_install.projects.pnpmfile_hook_override = pnpmfile_hook;
            base_install.projects.workspace_projects_override = workspace_projects_override;
            base_install
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
    ) -> miette::Result<&'static Config> {
        let node_linker = self.install_args.materialization.node_linker.map_or(
            base_config.node_linker,
            NodeLinkerArg::into_config,
        );
        let mut deploy_config = create_deploy_install_config(base_config, deploy_dir, node_linker);
        deploy_config.prefer_frozen_lockfile = frozen_lockfile;
        // pnpm's deploy forwards `--force` into the install, where it
        // bypasses the installability check so optional dependencies of
        // every platform are materialized (see `Config::force`).
        deploy_config.force = self.install_args.materialization.force;
        // `source_hooks` is the whole of the pnpmfile this install runs.
        // With none to run there is nothing left to discover either: the
        // install must not fall back to looking next to the deployed
        // manifest, where `copy_project` may have left the deployed
        // project's own pnpmfile.
        deploy_config.ignore_pnpmfile = ignore_pnpmfile;
        deploy_config.disable_agent_skills();
        // The deploy hook injects only the workspace packages a manifest
        // declares. A `workspace:` peer that the install auto-installs has
        // no declaring manifest, and must still be copied into the deploy.
        // This applies to every workspace package of the deploy install,
        // whatever the workspace sets; a legacy deploy saves no lockfile,
        // so the setting is not recorded anywhere.
        if let DeployInstallMode::Legacy {
            workspace_projects,
            source_lockfile_dir,
        } = &mode
        {
            deploy_config.inject_workspace_packages = true;
            let lockfile_dir = deploy_config.lockfile_dir_for(deploy_dir).to_path_buf();
            approve_legacy_workspace_project_builds(
                &mut deploy_config.allow_builds,
                workspace_projects,
                &lockfile_dir,
                source_lockfile_dir,
            )?;
        }
        apply_shared_deploy_config(&mut deploy_config, deploy_dir, mode);
        Ok(Config::leak(deploy_config))
    }
}
