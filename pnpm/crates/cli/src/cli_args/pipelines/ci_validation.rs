use super::{
    Arc, Config, DedicatedProjects, InstallArgs, InstallFamilyPlan, InstallFamilySelection,
    PathBuf, Reporter, ThrottledClient,
    dedicated_runs::init_dedicated_project_state,
    install::{NodeInstallJob, discover_dedicated_workspace_projects, init_shared_state},
};

impl NodeInstallJob {
    fn cleans_before_install(&self) -> bool {
        self.args.materialization.clean_install
            && !self.args.materialization.dry_run
            && !self.args.lockfile.only
    }

    pub(super) async fn validate_and_clean_if_needed<Reporter: self::Reporter + 'static>(
        &self,
        plan: &InstallFamilyPlan,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<()> {
        if !self.cleans_before_install() {
            return Ok(());
        }
        self.validate_plan::<Reporter>(plan, lockfile, http_client).await?;
        crate::cli_args::clean::run_for_ci(self.cfg, &self.prefix, plan)
    }

    async fn validate_plan<Reporter: self::Reporter + 'static>(
        &self,
        plan: &InstallFamilyPlan,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                validate_dedicated_projects::<Reporter>(
                    &self.args,
                    self.cfg,
                    projects,
                    self.require_lockfile,
                    http_client,
                )
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                self.validate_with_shared_state::<Reporter>(Some(selection), lockfile, http_client)
                    .await
            }
            InstallFamilyPlan::Single => {
                self.validate_single_plan::<Reporter>(lockfile, http_client).await
            }
        }
    }

    async fn validate_single_plan<Reporter: self::Reporter + 'static>(
        &self,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<()> {
        if !self.cfg.shares_one_lockfile()
            && let Some(workspace_dir) = self.cfg.workspace_dir.as_deref()
        {
            let projects = discover_dedicated_workspace_projects(self.cfg, workspace_dir)?;
            return validate_dedicated_projects::<Reporter>(
                &self.args,
                self.cfg,
                &projects,
                self.require_lockfile,
                http_client,
            )
            .await;
        }
        self.validate_with_shared_state::<Reporter>(None, lockfile, http_client).await
    }

    async fn validate_with_shared_state<Reporter: self::Reporter + 'static>(
        &self,
        selection: Option<&InstallFamilySelection>,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<()> {
        let state = init_shared_state(
            self.manifest_path.clone(),
            self.cfg,
            self.require_lockfile,
            lockfile.take(),
            Arc::clone(http_client),
        )?;
        let validated = Box::pin(self.args.validate_frozen::<Reporter>(&state, selection)).await;
        *lockfile = Some(state.lockfile);
        validated
    }
}

async fn validate_dedicated_projects<Reporter: self::Reporter + 'static>(
    args: &InstallArgs,
    cfg: &Config,
    projects: &DedicatedProjects,
    require_lockfile: bool,
    http_client: &Arc<ThrottledClient>,
) -> miette::Result<()> {
    let caches = crate::state::DedicatedCaches::default();
    for project_dir in projects.dependencies.keys() {
        let state = init_dedicated_project_state(
            cfg,
            project_dir,
            projects.names.get(project_dir).map(String::as_str),
            require_lockfile,
            Some(Arc::clone(http_client)),
        )?
        .into_dedicated_project(&caches, None, Some(projects.workspace.clone()));
        Box::pin(args.validate_frozen::<Reporter>(&state, None)).await?;
    }
    if !args.materialization.dry_run && projects.covers_workspace {
        let project_dirs: Vec<PathBuf> = projects.dependencies
            .keys()
            .cloned()
            .collect();
        pnpm_package_manager::verify_patches_against_project_lockfiles::<Reporter>(
            cfg,
            &project_dirs,
        )
        .map_err(miette::Report::new)?;
    }
    Ok(())
}
