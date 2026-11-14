use super::{
    Arc, Config, DedicatedProjects, InstallArgs, InstallFamilyPlan, InstallFamilySelection,
    PathBuf, Reporter, ThrottledClient,
    dedicated_runs::{discover_dedicated_workspace_projects, init_dedicated_project_state},
    install::{NodeInstallJob, init_shared_state},
};
use futures_util::stream::{StreamExt as _, TryStreamExt as _};
use pnpm_package_manager::{FrozenVirtualStoreClean, PreparedFrozenInstall};
use std::{collections::HashMap, sync::Mutex};

pub(super) struct PreparedDedicatedProject {
    pub(super) lockfile: pnpm_lockfile::LazyLockfile,
    pub(super) install: PreparedFrozenInstall,
}

#[derive(Default)]
pub(super) struct PreparedCiValidation {
    pub(super) shared: Option<PreparedFrozenInstall>,
    pub(super) per_project: Arc<Mutex<HashMap<PathBuf, PreparedDedicatedProject>>>,
    pub(super) dedicated_projects: Option<DedicatedProjects>,
}

impl NodeInstallJob {
    fn cleans_before_install(&self) -> bool {
        self.args.materialization.entry_mode.is_clean_install()
            && !self.args.materialization.dry_run
            && !self.args.lockfile.only
    }

    pub(super) async fn validate_and_clean_if_needed<Reporter: self::Reporter + 'static>(
        &self,
        plan: &InstallFamilyPlan,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<PreparedCiValidation> {
        if !self.cleans_before_install() {
            return Ok(PreparedCiValidation::default());
        }
        let prepared = self.validate_plan::<Reporter>(plan, lockfile, http_client).await?;
        crate::cli_args::clean::run_for_ci(self.cfg, &self.prefix, plan)?;
        Ok(prepared)
    }

    async fn validate_plan<Reporter: self::Reporter + 'static>(
        &self,
        plan: &InstallFamilyPlan,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<PreparedCiValidation> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                let per_project = validate_dedicated_projects::<Reporter>(
                    &self.args,
                    self.cfg,
                    projects,
                    self.require_lockfile,
                    http_client,
                )
                .await?;
                Ok(PreparedCiValidation { per_project, ..Default::default() })
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(PreparedCiValidation::default());
                }
                let shared = self.validate_with_shared_state::<Reporter>(
                    Some(selection),
                    lockfile,
                    http_client,
                )
                .await?;
                Ok(PreparedCiValidation { shared, ..Default::default() })
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
    ) -> miette::Result<PreparedCiValidation> {
        if !self.cfg.shares_one_lockfile()
            && let Some(workspace_dir) = self.cfg.workspace_dir.as_deref()
        {
            let projects = discover_dedicated_workspace_projects(self.cfg, workspace_dir)?;
            let per_project = validate_dedicated_projects::<Reporter>(
                &self.args,
                self.cfg,
                &projects,
                self.require_lockfile,
                http_client,
            )
            .await?;
            return Ok(PreparedCiValidation {
                per_project,
                dedicated_projects: Some(projects),
                ..Default::default()
            });
        }
        let shared =
            self.validate_with_shared_state::<Reporter>(None, lockfile, http_client).await?;
        Ok(PreparedCiValidation { shared, ..Default::default() })
    }

    async fn validate_with_shared_state<Reporter: self::Reporter + 'static>(
        &self,
        selection: Option<&InstallFamilySelection>,
        lockfile: &mut Option<pnpm_lockfile::LazyLockfile>,
        http_client: &Arc<ThrottledClient>,
    ) -> miette::Result<Option<PreparedFrozenInstall>> {
        let virtual_store_clean =
            crate::cli_args::clean::ci_virtual_store_clean(self.cfg, &self.prefix);
        let state = init_shared_state(
            self.manifest_path.clone(),
            self.cfg,
            self.require_lockfile,
            lockfile.take(),
            Arc::clone(http_client),
        )?;
        let prepared =
            Box::pin(self.args.prepare_for_ci::<Reporter>(&state, selection, virtual_store_clean))
                .await;
        *lockfile = Some(state.lockfile);
        prepared
    }
}

async fn validate_dedicated_projects<Reporter: self::Reporter + 'static>(
    args: &InstallArgs,
    cfg: &Config,
    projects: &DedicatedProjects,
    require_lockfile: bool,
    http_client: &Arc<ThrottledClient>,
) -> miette::Result<Arc<Mutex<HashMap<PathBuf, PreparedDedicatedProject>>>> {
    let caches = crate::state::DedicatedCaches::default();
    let per_project = Arc::new(Mutex::new(HashMap::with_capacity(projects.dependencies.len())));
    let concurrency = usize::try_from(cfg.workspace_concurrency).unwrap_or(usize::MAX).max(1);
    futures_util::stream::iter(projects.dependencies.keys())
        .map(Ok)
        .try_for_each_concurrent(Some(concurrency), |project_dir| {
            let caches = &caches;
            let per_project = Arc::clone(&per_project);
            async move {
                let state = init_dedicated_project_state(
                    cfg,
                    project_dir,
                    projects.names.get(project_dir).map(String::as_str),
                    require_lockfile,
                    Some(Arc::clone(http_client)),
                )?
                .into_dedicated_project(caches, None, Some(projects.workspace.clone()));
                if let Some(install) = Box::pin(args.prepare_for_ci::<Reporter>(
                    &state,
                    None,
                    FrozenVirtualStoreClean::RemovesVirtualStore,
                ))
                .await?
                {
                    per_project
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(
                            project_dir.clone(),
                            PreparedDedicatedProject { lockfile: state.lockfile, install },
                        );
                }
                Ok::<(), miette::Report>(())
            }
        })
        .await?;
    verify_dedicated_patches_if_whole_workspace::<Reporter>(args, cfg, projects)?;
    Ok(per_project)
}

fn verify_dedicated_patches_if_whole_workspace<Reporter: self::Reporter>(
    args: &InstallArgs,
    cfg: &Config,
    projects: &DedicatedProjects,
) -> miette::Result<()> {
    if args.materialization.dry_run || args.lockfile.fix || !projects.covers_workspace {
        return Ok(());
    }
    let project_dirs: Vec<PathBuf> = projects.dependencies
        .keys()
        .cloned()
        .collect();
    pnpm_package_manager::verify_patches_against_project_lockfiles::<Reporter>(cfg, &project_dirs)
        .map_err(miette::Report::new)
}
