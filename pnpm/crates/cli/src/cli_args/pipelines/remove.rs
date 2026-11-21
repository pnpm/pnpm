use super::{
    Config,
    Context,
    DedicatedProjectRuns,
    EcosystemPackageSpecifier,
    InstallFamily,
    InstallFamilyPlan,
    Path,
    PathBuf,
    RemoveArgs,
    Reporter,
    RuntimePolicy,
    State,
    anchor_active_project,
    mutation::node_mutation_metadata_paths,
    prepare_root_config,
    select_install_family,
};
pub(crate) struct RemovePipeline {
    pub(crate) args: RemoveArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl RemovePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(mut self) -> miette::Result<()> {
        let root_config = (&*self.manifest_path, &mut *self.cfg, &*self.config_root);
        prepare_root_config::<Reporter>(root_config, (false, RuntimePolicy::Config(false))).await?;
        let packages = self.args.take_ecosystem_packages(self.cfg)?;
        let family = select_install_family::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            false,
        )?;
        if matches!(family.plan, InstallFamilyPlan::Single)
            && !self.cfg.shares_one_lockfile()
            && self.cfg.workspace_dir.is_some()
        {
            anchor_active_project(self.cfg, &self.manifest_path);
        }
        if packages.is_empty() {
            if let Some(unmatched) = family.unmatched {
                return Err(unmatched.report());
            }
            return Self::run_plan::<Reporter>(
                self.args,
                self.cfg,
                self.manifest_path,
                family.plan,
            )
            .await;
        }
        self.run_ecosystems::<Reporter>(family, packages).await
    }

    async fn run_ecosystems<Reporter: self::Reporter + 'static>(
        self,
        mut family: InstallFamily,
        packages: Vec<EcosystemPackageSpecifier>,
    ) -> miette::Result<()> {
        let args = self.args;
        let mut dirs = family.standalone_dirs(self.cfg, &self.prefix).await?;
        if let Some(scope) = &family.scope {
            dirs.extend(scope.selected.iter().cloned());
        } else {
            dirs.push(self.prefix.clone());
        }
        if dirs.is_empty() {
            return Ok(());
        }
        let plan = args.ecosystem_plan::<Reporter>(self.cfg, &self.prefix, &dirs, packages).await?;
        if args.package_names.is_empty() {
            return plan.run().await;
        }
        let metadata = node_remove_metadata_paths(self.cfg, &self.manifest_path, &family.plan);
        let node = Self::run_plan::<Reporter>(args, self.cfg, self.manifest_path, family.plan);
        plan.with_task(pnpm_install_coordinator::InstallTask::in_place(metadata, node))
            .run()
            .await
    }

    async fn run_plan<Reporter: self::Reporter + 'static>(
        args: RemoveArgs,
        cfg: &'static Config,
        manifest_path: PathBuf,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                // Dedicated per-project lockfiles: remove the packages from
                // each selected project independently.
                DedicatedProjectRuns {
                    config: cfg,
                    projects: *projects,
                    require_lockfile: false,
                    http_client: None,
                    prune_excludes: true,
                    sync_injected_deps: false,
                    pipelined: false,
                }
                .run::<Reporter, _, _>(|state| Box::pin(args.clone().run::<Reporter>(state)))
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await
            }
        }
    }
}

fn node_remove_metadata_paths(
    config: &Config,
    manifest_path: &Path,
    plan: &InstallFamilyPlan,
) -> Vec<PathBuf> {
    match plan {
        InstallFamilyPlan::Single => node_mutation_metadata_paths(config, manifest_path),
        InstallFamilyPlan::Shared(selection) => selection.projects
            .iter()
            .filter(|project| selection.selected_dirs.contains(&project.root_dir))
            .flat_map(|project| node_mutation_metadata_paths(config, project.manifest.path()))
            .collect(),
        InstallFamilyPlan::PerProject(projects) => projects.dependencies
            .keys()
            .flat_map(|dir| {
                let mut local = config.clone();
                local.anchor_dedicated_project(dir, projects.names.get(dir).map(String::as_str));
                node_mutation_metadata_paths(
                    &local,
                    &pnpm_package_manifest::project_manifest_path(dir),
                )
            })
            .collect(),
    }
}
