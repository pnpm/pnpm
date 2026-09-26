use super::{
    Arc, Config, Context, DedicatedProjectRuns, InstallArgs, InstallFamily, InstallFamilyPlan,
    Path, PathBuf, Reporter, RuntimePolicy, State, ThrottledClient, dedicated_project_name,
    discover_workspace_projects, ecosystem_install, init_dedicated_project_state,
    prepare_root_config, project_names, prune_after_dedicated_installs, select_install_family,
    sync_dedicated_injected_deps,
};

/// The reporter-generic body of `pacquet install`: it threads one `Reporter`
/// type through config-dependency sync, the `updateConfig` hooks, and the
/// install itself. Lifting it out of the dispatch keeps the three
/// `ReporterType` arms to a single line each.
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
        self.run_with_config::<Reporter>().await
            .map(|_| ())
    }

    pub(crate) async fn run_with_config<Reporter: self::Reporter + 'static>(
        self,
    ) -> miette::Result<&'static Config> {
        let root_config = (&*self.manifest_path, &mut *self.cfg, &*self.config_root);
        prepare_root_config::<Reporter>(
            root_config,
            (self.frozen_lockfile, RuntimePolicy::Config(self.args.materialization.no_runtime)),
        )
        .await?;
        // Built ahead of project discovery so a run that is certain to
        // read the wanted lockfile parses it on a background thread
        // while discovery walks the workspace. Certain means the fast
        // "Already up to date" return cannot fire: it is off under
        // `--frozen-lockfile` / `--force`, and it requires a workspace
        // state from a previous install — a workspace with none on
        // disk (a lockfile-only workflow never writes one) always
        // reaches the full pipeline. A `--fix-lockfile` run reads
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
        let family = select_install_family::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            certain_full_install,
        )?;
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

        self.run_prepared::<Reporter>(family, lockfile).await
    }

    async fn run_prepared<Reporter: self::Reporter + 'static>(
        self,
        family: InstallFamily,
        lockfile: Option<pnpm_lockfile::LazyLockfile>,
    ) -> miette::Result<&'static Config> {
        let http_client =
            State::new_http_client(self.cfg).wrap_err("initialize the install network")?;
        if !ecosystem_install::is_enabled(self.cfg) {
            run_node_install::<Reporter>(
                family.plan,
                self.args,
                self.cfg,
                self.manifest_path,
                self.require_lockfile,
                lockfile,
                http_client,
            )
            .await?;
            return Ok(self.cfg);
        }
        self.run_with_ecosystems::<Reporter>(family, lockfile, http_client).await
    }

    async fn run_with_ecosystems<Reporter: self::Reporter + 'static>(
        self,
        family: InstallFamily,
        lockfile: Option<pnpm_lockfile::LazyLockfile>,
        http_client: Arc<ThrottledClient>,
    ) -> miette::Result<&'static Config> {
        let ecosystem = ecosystem_install::plan::<Reporter>(
            ecosystem_install::InstallContext {
                config: self.cfg,
                http_client: Arc::clone(&http_client),
                lockfile_only: self.args.lockfile.only,
                frozen_lockfile: self.frozen_lockfile,
            },
            self.config_root,
            &self.prefix,
            &self.args.dependency_options,
            family.scope.as_ref(),
        )
        .await?;
        // A selector that named no npm project may have named a Python one.
        if let Some(unmatched) = family.unmatched
            && ecosystem.python.selected == 0
        {
            return Err(unmatched.counting(ecosystem.python.discovered).report());
        }
        let node_install = run_node_install::<Reporter>(
            family.plan,
            self.args,
            self.cfg,
            self.manifest_path,
            self.require_lockfile,
            lockfile,
            http_client,
        );
        ecosystem.plan
            .with_task(pnpm_install_coordinator::InstallTask::in_place(Vec::new(), node_install))
            .run()
            .await?;
        Ok(self.cfg)
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

async fn run_node_install<Reporter: self::Reporter + 'static>(
    plan: InstallFamilyPlan,
    args: InstallArgs,
    cfg: &'static Config,
    manifest_path: PathBuf,
    require_lockfile: bool,
    lockfile: Option<pnpm_lockfile::LazyLockfile>,
    http_client: Arc<ThrottledClient>,
) -> miette::Result<()> {
    match plan {
        InstallFamilyPlan::PerProject(projects) => {
            DedicatedProjectRuns {
                config: cfg,
                projects,
                require_lockfile,
                http_client: Some(Arc::clone(&http_client)),
                prune_excludes: !args.materialization.dry_run,
                sync_injected_deps: !(args.lockfile.only || args.materialization.dry_run),
            }
            .run(|state| Box::pin(args.clone().run::<Reporter>(state)))
            .await
        }
        InstallFamilyPlan::Shared(selection) => {
            if selection.selected_dirs.is_empty() {
                return Ok(());
            }
            let state = init_shared_state(
                manifest_path,
                cfg,
                require_lockfile,
                lockfile,
                Arc::clone(&http_client),
            )?;
            Box::pin(args.run_selected::<Reporter>(state, *selection)).await
        }
        InstallFamilyPlan::Single => {
            run_single_node_install::<Reporter>(
                args,
                cfg,
                manifest_path,
                require_lockfile,
                lockfile,
                http_client,
            )
            .await
        }
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

/// `sharedWorkspaceLockfile: false` workspace install: one independent
/// single-project install per workspace project — each gets its own
/// `pnpm-lock.yaml`, `node_modules`, and virtual store, mirroring
/// pnpm's dedicated-lockfile per-project loop in its recursive
/// dispatch. The workspace root participates when it has a manifest,
/// matching the project set a shared-lockfile workspace install covers.
pub(super) async fn run_dedicated_lockfile_workspace_install<Reporter: self::Reporter + 'static>(
    args: &super::super::install::InstallArgs,
    cfg: &Config,
    workspace_root: &Path,
    require_lockfile: bool,
    http_client: Arc<ThrottledClient>,
) -> miette::Result<()> {
    let (projects, _patterns) = discover_workspace_projects(workspace_root, cfg)?;
    let mut names = project_names(cfg, &projects);
    let normalized_root = pnpm_fs::lexical_normalize(workspace_root);
    let mut project_dirs: Vec<PathBuf> = Vec::with_capacity(projects.len() + 1);
    if pnpm_package_manifest::project_manifest_path(workspace_root).is_file()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_root)
    {
        project_dirs.push(workspace_root.to_path_buf());
        // The root is installed alongside the discovered projects but is
        // not one of them, so its name is not in the map yet.
        if let Some(name) = dedicated_project_name(cfg, workspace_root) {
            names.insert(workspace_root.to_path_buf(), name);
        }
    }
    project_dirs.extend(projects.into_iter().map(|project| project.root_dir));
    // One `Config::leak` per project: `State::init` needs a
    // `&'static Config`, and a leaked shared reference can't be
    // reclaimed for the next iteration. The leak is bounded by the
    // project count, happens once per CLI invocation, and is
    // reclaimed at process exit — the same lifetime deploy's derived
    // install config has.
    for project_dir in &project_dirs {
        let state = init_dedicated_project_state(
            cfg,
            project_dir,
            names.get(project_dir).map(String::as_str),
            require_lockfile,
            Some(Arc::clone(&http_client)),
        )?;
        Box::pin(args.clone().run::<Reporter>(state)).await?;
    }
    if !args.materialization.dry_run {
        prune_after_dedicated_installs(cfg)?;
    }
    if !(args.lockfile.only || args.materialization.dry_run) {
        sync_dedicated_injected_deps(cfg, &project_dirs, &names)?;
    }
    Ok(())
}

async fn run_single_node_install<Reporter: self::Reporter + 'static>(
    args: InstallArgs,
    cfg: &'static Config,
    manifest_path: PathBuf,
    require_lockfile: bool,
    lockfile: Option<pnpm_lockfile::LazyLockfile>,
    http_client: Arc<ThrottledClient>,
) -> miette::Result<()> {
    if !cfg.shares_one_lockfile()
        && let Some(workspace_dir) = cfg.workspace_dir.clone()
    {
        return run_dedicated_lockfile_workspace_install::<Reporter>(
            &args,
            cfg,
            &workspace_dir,
            require_lockfile,
            Arc::clone(&http_client),
        )
        .await;
    }
    let state = init_shared_state(
        manifest_path,
        cfg,
        require_lockfile,
        lockfile,
        Arc::clone(&http_client),
    )?;
    Box::pin(args.run::<Reporter>(state)).await
}
