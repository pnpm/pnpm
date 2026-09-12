use super::{
    AddArgs, Arc, BTreeMap, Config, Context, DedicatedProjectRuns, DeployArgs, InstallFamilyPlan,
    Path, PathBuf, RemoveArgs, Reporter, State, UpdateArgs, UpdateChangesetContext,
    anchor_active_project, config_deps, dedicated_project_name, ecosystem_add, ecosystem_install,
    init_shared_state, select_install_family_plan,
};

pub(crate) struct AddPipeline {
    pub(crate) args: AddArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    /// [`AddArgs::parse_config_dependencies`]'s output, parsed by the dispatch
    /// before this pipeline scaffolds a manifest. `Some` exactly when
    /// `--config` was passed.
    pub(crate) config_dependencies: Option<BTreeMap<String, String>>,
    pub(crate) package_specifier_plan: crate::package_specifier::PackageSpecifierPlan,
}

impl AddPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        config_deps::prepare::<Reporter>(self.cfg, &self.config_root, false).await?;
        if !self.package_specifier_plan.ecosystem_packages.is_empty() {
            return run_add_with_ecosystems::<Reporter>(
                self.args,
                self.cfg,
                self.prefix,
                self.manifest_path,
                self.package_specifier_plan,
            )
            .await;
        }
        // `--config` targets the workspace's configuration dependencies, not
        // any project's manifest, so it bypasses project selection entirely.
        let plan = if self.config_dependencies.is_some() {
            InstallFamilyPlan::Single
        } else {
            select_install_family_plan::<Reporter>(
                self.cfg,
                &self.prefix,
                &self.manifest_path,
                self.recursive_sort,
                true,
                false,
            )?
        };
        self.run_plan::<Reporter>(plan).await
    }

    pub(super) async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                // Dedicated per-project lockfiles: add the packages to each
                // selected project independently.
                let workspace_packages = self.args.workspace_link_targets(self.cfg)?;
                DedicatedProjectRuns {
                    config: self.cfg,
                    projects,
                    require_lockfile: false,
                    http_client: None,
                }
                .run(|state| {
                    Box::pin(self.args.clone().run_with_link_targets::<Reporter>(
                        state,
                        None,
                        workspace_packages.as_ref(),
                    ))
                })
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                // Dedicated per-project lockfiles: `add` mutates only the
                // active project, whose outputs anchor at the project dir.
                // `--config` targets the workspace's configuration
                // dependencies, which stay workspace-anchored.
                if self.config_dependencies.is_none()
                    && !self.cfg.shares_one_lockfile()
                    && self.cfg.workspace_dir.is_some()
                {
                    anchor_active_project(self.cfg, &self.manifest_path);
                }
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(state, self.config_dependencies)).await
            }
        }
    }
}

async fn run_add_with_ecosystems<Reporter: self::Reporter + 'static>(
    args: AddArgs,
    cfg: &'static mut Config,
    prefix: PathBuf,
    manifest_path: PathBuf,
    package_specifier_plan: crate::package_specifier::PackageSpecifierPlan,
) -> miette::Result<()> {
    let has_node_packages = !package_specifier_plan.node_packages.is_empty();
    if !cfg.shares_one_lockfile() && cfg.workspace_dir.is_some() && has_node_packages {
        let manifest_dir =
            manifest_path.parent().expect("manifest path always has a parent dir").to_path_buf();
        let name = dedicated_project_name(cfg, &manifest_dir);
        cfg.anchor_dedicated_project(&manifest_dir, name.as_deref());
    }
    let http_client = State::new_http_client(cfg).wrap_err("initialize the add network")?;
    let cfg: &'static Config = cfg;
    let plan = ecosystem_add::plan::<Reporter>(
        ecosystem_install::InstallContext {
            config: cfg,
            http_client: Arc::clone(&http_client),
            lockfile_only: args.lockfile_only,
            frozen_lockfile: false,
        },
        prefix,
        package_specifier_plan.ecosystem_packages,
        &args,
        has_node_packages,
    )
    .await?;
    if !has_node_packages {
        return plan.run().await;
    }
    let metadata = node_add_metadata_paths(cfg, &manifest_path);
    let mut node_args = args;
    node_args.package_names = package_specifier_plan.node_packages;
    let node_install = async move {
        let state = init_shared_state(manifest_path, cfg, false, None, http_client)?;
        Box::pin(node_args.run::<Reporter>(state, None)).await
    };
    plan.with_task(pnpm_install_coordinator::InstallTask::in_place(metadata, node_install))
        .run()
        .await
}

pub(super) fn node_add_metadata_paths(config: &Config, manifest_path: &Path) -> Vec<PathBuf> {
    let project_dir = manifest_path.parent().expect("manifest path always has a parent dir");
    let mut paths = vec![
        manifest_path.to_path_buf(),
        config.lockfile_dir_for(project_dir).join(config.wanted_lockfile_name()),
        // The current lockfile remains project-local even with a global virtual store.
        config.virtual_store_dir.join(pnpm_lockfile::Lockfile::CURRENT_FILE_NAME),
        config.modules_dir.join(pnpm_modules_yaml::MODULES_FILENAME),
    ];
    if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        paths.push(workspace_dir.join("pnpm-workspace.yaml"));
    }
    paths
}

#[cfg(test)]
#[path = "add_metadata_paths_tests.rs"]
mod add_metadata_paths_tests;

pub(crate) struct UpdatePipeline {
    pub(crate) args: UpdateArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl UpdatePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        config_deps::prepare::<Reporter>(self.cfg, &self.config_root, false).await?;
        let plan = select_install_family_plan::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            false,
        )?;
        // An empty selection has nothing to update, and — like the shared
        // path — must not generate a changeset.
        match &plan {
            InstallFamilyPlan::PerProject(projects) if projects.is_empty() => {
                return Ok(());
            }
            InstallFamilyPlan::Shared(selection) if selection.selected_dirs.is_empty() => {
                return Ok(());
            }
            _ => {}
        }
        // Dedicated per-project lockfiles: the non-recursive command
        // mutates only the active project, whose outputs anchor at the
        // project dir.
        if matches!(plan, InstallFamilyPlan::Single)
            && !self.cfg.shares_one_lockfile()
            && self.cfg.workspace_dir.is_some()
        {
            anchor_active_project(self.cfg, &self.manifest_path);
        }
        let generate_changeset = if self.args.changeset {
            true
        } else if self.args.no_changeset {
            false
        } else {
            self.cfg.update_config.changeset.unwrap_or(false)
        };
        let changeset_context = generate_changeset
            .then(|| UpdateChangesetContext::capture(self.cfg, &self.manifest_path))
            .transpose()?;
        self.run_plan::<Reporter>(plan).await?;
        if let Some(changeset_context) = changeset_context {
            changeset_context.generate::<Reporter>()?;
        }
        Ok(())
    }
    pub(super) async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                DedicatedProjectRuns {
                    config: self.cfg,
                    projects,
                    require_lockfile: false,
                    http_client: None,
                }
                .run(|state| Box::pin(self.args.clone().run::<Reporter>(state)))
                .await?;
            }
            InstallFamilyPlan::Shared(selection) => {
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run_selected::<Reporter>(state, *selection)).await?;
            }
            InstallFamilyPlan::Single => {
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(state)).await?;
            }
        }
        Ok(())
    }
}

pub(crate) struct RemovePipeline {
    pub(crate) args: RemoveArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl RemovePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let RemovePipeline { args, cfg, config_root, prefix, manifest_path, recursive_sort } = self;
        config_deps::prepare::<Reporter>(cfg, &config_root, false).await?;
        let plan = select_install_family_plan::<Reporter>(
            cfg,
            &prefix,
            &manifest_path,
            recursive_sort,
            false,
            false,
        )?;
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                // Dedicated per-project lockfiles: remove the packages from
                // each selected project independently.
                DedicatedProjectRuns {
                    config: cfg,
                    projects,
                    require_lockfile: false,
                    http_client: None,
                }
                .run(|state| Box::pin(args.clone().run::<Reporter>(state)))
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                // Dedicated per-project lockfiles: the non-recursive command
                // mutates only the active project, whose outputs anchor at the
                // project dir.
                if !cfg.shares_one_lockfile() && cfg.workspace_dir.is_some() {
                    anchor_active_project(cfg, &manifest_path);
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await
            }
        }
    }
}

pub(crate) struct DeployPipeline {
    pub(crate) args: DeployArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
}

impl DeployPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir_ref: &Path,
    ) -> miette::Result<()> {
        let DeployPipeline { args, cfg, config_root } = self;
        config_deps::prepare::<Reporter>(cfg, &config_root, false).await?;
        let cfg: &'static Config = cfg;
        Box::pin(args.run::<Reporter>(cfg, dir_ref)).await
    }
}
