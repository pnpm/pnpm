use super::{
    Arc, Config, Context, InstallArgs, InstallFamily, InstallFamilyPlan, Path, PathBuf, Reporter,
    RuntimePolicy, State, ThrottledClient,
    dedicated_runs::{discover_dedicated_workspace_projects, run_dedicated_projects},
    ecosystem_install, prepare_root_config, select_install_family,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use tracing::Instrument as _;

/// A plain install outside a workspace found no project manifest. Only
/// `pnpm add` and `pnpm link` create one.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("No package.json found in {}", dir.display())]
#[diagnostic(code(ERR_PNPM_NO_PKG_MANIFEST))]
pub(crate) struct NoPkgManifest {
    #[error(not(source))]
    pub(crate) dir: PathBuf,
}

/// Fail a non-recursive install outside a workspace that has no manifest
/// to install from. A workspace root may lack a manifest.
///
/// Call it before anything the command writes, removes, or fetches.
pub(crate) fn ensure_project_manifest(
    cfg: &Config,
    manifest_path: &Path,
) -> Result<(), NoPkgManifest> {
    let dir = manifest_path.parent().expect("manifest path always has a parent dir");
    if cfg.recursive
        || cfg.workspace_dir.is_some()
        || pnpm_package_manifest::project_manifest_path(dir).is_file()
    {
        return Ok(());
    }
    Err(NoPkgManifest { dir: dir.to_path_buf() })
}

/// The reporter-generic body of `pacquet install`: it threads one `Reporter`
/// type through config-dependency sync, the `updateConfig` hooks, and the
/// install itself.
pub(crate) struct InstallPipeline {
    pub(crate) args: InstallArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    pub(crate) require_lockfile: bool,
    pub(crate) frozen_lockfile: bool,
}

impl InstallPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        Box::pin(self.run_with_config::<Reporter>()).await.map(|_| ())
    }

    pub(crate) async fn run_with_config<Reporter: self::Reporter + 'static>(
        self,
    ) -> miette::Result<&'static Config> {
        let root_config = (&*self.manifest_path, &mut *self.cfg, &*self.config_root);
        prepare_root_config::<Reporter>(
            root_config,
            (self.frozen_lockfile, RuntimePolicy::Config(self.args.materialization.no_runtime)),
        )
        .instrument(tracing::debug_span!(target: "pacquet::install", "prepare_config"))
        .await?;
        if self.args.finished_from_records_after_config(&self.prefix, self.cfg, Reporter::emit) {
            return Ok(self.cfg);
        }
        // Built ahead of project discovery so a run that is certain to
        // read the wanted lockfile parses it on a background thread
        // while discovery walks the workspace. A `--fix-lockfile` run reads
        // through the separate repair loader, which this prefetch does
        // not feed. Only the shared-lockfile arms consume this
        // lockfile; the per-project arms load their own.
        let lockfile = self.cfg
            .shares_one_lockfile()
            .then(|| State::lazy_lockfile(self.cfg, &self.manifest_path, self.require_lockfile));
        let certain_full_install = self.certain_full_install();
        if let Some(lockfile) = lockfile.as_ref()
            && !self.args.lockfile.only
            && certain_full_install
        {
            lockfile.prefetch();
        }
        let family = self.select_install_family::<Reporter>(certain_full_install)?;
        let installs_node = match &family.plan {
            InstallFamilyPlan::PerProject(projects) => !projects.is_empty(),
            InstallFamilyPlan::Shared(selection) => !selection.selected_dirs.is_empty(),
            InstallFamilyPlan::Single => true,
        };
        if !ecosystem_install::is_enabled(self.cfg) {
            if let Some(unmatched) = family.unmatched {
                return Err(unmatched.report());
            }
            if !installs_node {
                return Ok(self.cfg);
            }
        }

        Box::pin(self.run_prepared::<Reporter>(family, lockfile)).await
    }

    /// [`select_install_family`] inside the `discover_workspace_projects`
    /// span.
    fn select_install_family<Reporter: self::Reporter>(
        &self,
        certain_full_install: bool,
    ) -> miette::Result<InstallFamily> {
        let _span = tracing::debug_span!(target: "pacquet::install", "discover_workspace_projects")
            .entered();
        select_install_family::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            certain_full_install,
        )
    }

    async fn run_prepared<Reporter: self::Reporter + 'static>(
        self,
        family: InstallFamily,
        mut lockfile: Option<pnpm_lockfile::LazyLockfile>,
    ) -> miette::Result<&'static Config> {
        let http_client = {
            let _span =
                tracing::debug_span!(target: "pacquet::install", "build_http_client").entered();
            State::new_http_client(self.cfg).wrap_err("initialize the install network")?
        };
        if !ecosystem_install::is_enabled(self.cfg) {
            let SplitInstallPipeline { job, .. } = self.into_node_job();
            let cfg = job.cfg;
            let prepared = job.validate_and_clean_if_needed::<Reporter>(
                &family.plan,
                &mut lockfile,
                &http_client,
            )
            .await?;
            job.run::<Reporter>(family.plan, lockfile, http_client, prepared).await?;
            return Ok(cfg);
        }
        self.run_with_ecosystems::<Reporter>(family, lockfile, http_client).await
    }

    async fn run_with_ecosystems<Reporter: self::Reporter + 'static>(
        self,
        family: InstallFamily,
        mut lockfile: Option<pnpm_lockfile::LazyLockfile>,
        http_client: Arc<ThrottledClient>,
    ) -> miette::Result<&'static Config> {
        let SplitInstallPipeline { job, config_root, frozen_lockfile } = self.into_node_job();
        let cfg = job.cfg;
        let ecosystem = ecosystem_install::plan::<Reporter>(
            ecosystem_install::InstallContext {
                config: cfg,
                http_client: Arc::clone(&http_client),
                lockfile_only: job.args.lockfile.only,
                frozen_lockfile,
            },
            config_root,
            &job.prefix,
            &job.args.dependency_options,
            family.scope.as_ref(),
        )
        .await?;
        // A selector that named no npm project may have named a Python one.
        if let Some(unmatched) = family.unmatched
            && ecosystem.python.selected == 0
        {
            return Err(unmatched.counting(ecosystem.python.discovered).report());
        }
        let prepared =
            job.validate_and_clean_if_needed::<Reporter>(&family.plan, &mut lockfile, &http_client)
                .await?;
        let node_install = job.run::<Reporter>(family.plan, lockfile, http_client, prepared);
        ecosystem.plan
            .with_task(pnpm_install_coordinator::InstallTask::in_place(Vec::new(), node_install))
            .run()
            .await?;
        Ok(cfg)
    }

    fn into_node_job(self) -> SplitInstallPipeline {
        let InstallPipeline {
            args,
            cfg,
            config_root,
            prefix,
            manifest_path,
            require_lockfile,
            frozen_lockfile,
            ..
        } = self;
        SplitInstallPipeline {
            job: NodeInstallJob { args, cfg, prefix, manifest_path, require_lockfile },
            config_root,
            frozen_lockfile,
        }
    }

    /// Whether the fast "Already up to date" return cannot fire, so the run
    /// is certain to read the wanted lockfile.
    fn certain_full_install(&self) -> bool {
        self.cfg.shares_one_lockfile() && {
            let manifest_dir =
                self.manifest_path.parent().expect("manifest path always has a parent dir");
            let lockfile_dir = self.cfg.lockfile_dir_for(manifest_dir);
            self.frozen_lockfile
                || self.cfg.force
                || !pnpm_workspace_state::get_file_path(lockfile_dir).is_file()
        }
    }
}

struct SplitInstallPipeline {
    job: NodeInstallJob,
    config_root: PathBuf,
    frozen_lockfile: bool,
}

pub(super) struct NodeInstallJob {
    pub(super) args: InstallArgs,
    pub(super) cfg: &'static Config,
    pub(super) prefix: PathBuf,
    pub(super) manifest_path: PathBuf,
    pub(super) require_lockfile: bool,
}

impl NodeInstallJob {
    async fn run<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
        lockfile: Option<pnpm_lockfile::LazyLockfile>,
        http_client: Arc<ThrottledClient>,
        prepared: super::ci_validation::PreparedCiValidation,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                run_dedicated_projects::<Reporter>(
                    &self.args,
                    self.cfg,
                    *projects,
                    self.require_lockfile,
                    http_client,
                    prepared.per_project,
                )
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let state = init_shared_state(
                    self.manifest_path,
                    self.cfg,
                    self.require_lockfile,
                    lockfile,
                    Arc::clone(&http_client),
                )?;
                Box::pin(self.args.run_selected_prepared::<Reporter>(
                    state,
                    *selection,
                    prepared.shared,
                ))
                .await
            }
            InstallFamilyPlan::Single => {
                self.run_single::<Reporter>(lockfile, http_client, prepared).await
            }
        }
    }

    async fn run_single<Reporter: self::Reporter + 'static>(
        self,
        lockfile: Option<pnpm_lockfile::LazyLockfile>,
        http_client: Arc<ThrottledClient>,
        prepared: super::ci_validation::PreparedCiValidation,
    ) -> miette::Result<()> {
        if !self.cfg.shares_one_lockfile()
            && let Some(workspace_dir) = self.cfg.workspace_dir.clone()
        {
            let projects = match prepared.dedicated_projects {
                Some(projects) => projects,
                None => discover_dedicated_workspace_projects(self.cfg, &workspace_dir)?,
            };
            return run_dedicated_projects::<Reporter>(
                &self.args,
                self.cfg,
                projects,
                self.require_lockfile,
                Arc::clone(&http_client),
                prepared.per_project,
            )
            .await;
        }
        let state = init_shared_state(
            self.manifest_path,
            self.cfg,
            self.require_lockfile,
            lockfile,
            Arc::clone(&http_client),
        )?;
        Box::pin(self.args.run_prepared::<Reporter>(state, prepared.shared)).await
    }
}

/// [`State::init`], consuming the pipeline's pre-built lockfile when
/// there is one so an already-running prefetch isn't thrown away.
pub(super) fn init_shared_state(
    manifest_path: PathBuf,
    config: &'static Config,
    require_lockfile: bool,
    lockfile: Option<pnpm_lockfile::LazyLockfile>,
    http_client: Arc<ThrottledClient>,
) -> miette::Result<State> {
    let lockfile =
        lockfile.unwrap_or_else(|| State::lazy_lockfile(config, &manifest_path, require_lockfile));
    State::init_with_lockfile_and_http_client(manifest_path, config, lockfile, http_client)
        .wrap_err("initialize the state")
}
