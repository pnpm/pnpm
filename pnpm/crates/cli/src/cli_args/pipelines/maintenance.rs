use super::{
    Config, Context, DedicatedProjectRuns, DedicatedProjects, DedupeArgs, InstallFamilyPlan, Path,
    PathBuf, PruneArgs, Reporter, RuntimePolicy, State, anchor_active_project, dedupe,
    prepare_root_config, resolve_bool_override, select_install_family_plan,
};

/// The reporter-generic body of `pacquet dedupe`: snapshots the lockfile
/// (when `--check`), runs config-dependency installation and `updateConfig`
/// hooks, then dispatches to the install pipeline. The snapshot wraps the
/// entire pipeline so any lockfile write made by config-deps is also covered
/// by the check gate.
pub(crate) struct DedupePipeline {
    pub(crate) args: DedupeArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl DedupePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let lockfile_path = self.config_root.join(self.cfg.wanted_lockfile_name());

        // Snapshot before any config-dep writes so --check detects lockfile
        // changes made by config-dependency syncing as well.
        let existing =
            if self.args.check { dedupe::read_lockfile_snapshot(&lockfile_path)? } else { None };
        let guard =
            self.args.check.then(|| dedupe::LockfileGuard::new(existing.clone(), &lockfile_path));

        let root_config = (&*self.manifest_path, &mut *self.cfg, &*self.config_root);
        prepare_root_config::<Reporter>(
            root_config,
            (false, RuntimePolicy::Config(self.args.no_runtime)),
        )
        .await?;
        let plan = select_install_family_plan::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            false,
        )?;
        self.run_plan::<Reporter>(plan, lockfile_path, existing, guard).await
    }

    pub(super) async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
        lockfile_path: PathBuf,
        existing: Option<String>,
        guard: Option<dedupe::LockfileGuard>,
    ) -> miette::Result<()> {
        let cfg: &'static Config = self.cfg;
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                run_dedicated_dedupe::<Reporter>(
                    self.args,
                    cfg,
                    *projects,
                    &lockfile_path,
                    existing,
                    guard,
                )
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(
                    state,
                    existing,
                    guard,
                    &lockfile_path,
                    Some(&selection),
                ))
                .await
            }
            InstallFamilyPlan::Single => {
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(state, existing, guard, &lockfile_path, None))
                    .await
            }
        }
    }
}

async fn dedupe_dedicated_project<Reporter: self::Reporter + 'static>(
    args: DedupeArgs,
    state: State,
    root_lockfile_path: &Path,
    root_existing: Option<&str>,
) -> miette::Result<()> {
    let lockfile_path = state.lockfile_dir().join(state.config.wanted_lockfile_name());
    let existing = if args.check {
        if lockfile_path == root_lockfile_path {
            root_existing.map(str::to_string)
        } else {
            dedupe::read_lockfile_snapshot(&lockfile_path)?
        }
    } else {
        None
    };
    let guard = args.check.then(|| dedupe::LockfileGuard::new(existing.clone(), &lockfile_path));
    Box::pin(args.run::<Reporter>(state, existing, guard, &lockfile_path, None)).await
}

/// The reporter-generic body of `pacquet prune`: runs config-deps and
/// `updateConfig` hooks first, then applies prune-specific config
/// overrides (`modules_cache_max_age`, `ignore_scripts`) on the
/// post-hook config, and finally dispatches to the install pipeline.
/// The overrides must come after hooks because `updateConfig` can
/// mutate `Config` fields (including `modules_dir` /
/// `install_state_dir`), and the CLI `--ignore-scripts` flag must win
/// over any hook-set value.
pub(crate) struct PrunePipeline {
    pub(crate) args: PruneArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl PrunePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let root_config = (&*self.manifest_path, &mut *self.cfg, &*self.config_root);
        prepare_root_config::<Reporter>(root_config, (false, RuntimePolicy::Config(false))).await?;
        // Validate path containment AFTER hooks: updateConfig can mutate
        // modules_dir / install_state_dir via WorkspaceSettings::apply_to,
        // so the check must use the final (post-hook) config values.
        // The install pipeline's prune_target_within_modules also validates
        // VSD containment, but only at sweep time; this earlier check
        // catches a misconfigured modules_dir itself (e.g. an absolute
        // path outside the workspace) before any destructive work begins.
        //
        // `config_root` is `cfg.workspace_dir` when present, or the
        // canonicalized `--dir` otherwise — a meaningful containment
        // boundary in both cases.
        ensure_modules_dir_within_config_root(&self.cfg.modules_dir, &self.config_root)?;
        // Apply prune-specific overrides after hooks so that:
        // - `modules_cache_max_age = 0` forces the virtual-store sweep
        //   on the final (post-hook) config paths.
        // - `--ignore-scripts` from the CLI wins over any value the
        //   hooks set via `WorkspaceSettings::apply_to`.
        self.cfg.modules_cache_max_age = 0;
        self.cfg.ignore_scripts = resolve_bool_override(
            self.args.ignore_scripts,
            self.args.no_ignore_scripts,
            self.cfg.ignore_scripts,
        );
        let plan = select_install_family_plan::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            false,
        )?;
        self.run_plan::<Reporter>(plan).await
    }

    async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                run_dedicated_prune::<Reporter>(self.args, self.cfg, *projects, self.config_root)
                    .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let state = State::init(self.manifest_path, self.cfg, false)
                    .wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(state, Some(&selection))).await
            }
            InstallFamilyPlan::Single => {
                if !self.cfg.shares_one_lockfile() && self.cfg.workspace_dir.is_some() {
                    anchor_active_project(self.cfg, &self.manifest_path);
                    ensure_modules_dir_within_config_root(
                        &self.cfg.modules_dir,
                        &self.config_root,
                    )?;
                }
                let state = State::init(self.manifest_path, self.cfg, false)
                    .wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(state, None)).await
            }
        }
    }
}

async fn run_dedicated_prune<Reporter: self::Reporter + 'static>(
    args: PruneArgs,
    cfg: &'static Config,
    projects: DedicatedProjects,
    config_root: PathBuf,
) -> miette::Result<()> {
    for project_dir in projects.dependencies.keys() {
        let mut project_config = cfg.clone();
        project_config.anchor_dedicated_project(
            project_dir,
            projects.names.get(project_dir).map(String::as_str),
        );
        ensure_modules_dir_within_config_root(&project_config.modules_dir, &config_root)?;
    }
    DedicatedProjectRuns {
        config: cfg,
        projects,
        require_lockfile: false,
        http_client: Some(State::new_http_client(cfg)?),
        prune_excludes: false,
        sync_injected_deps: false,
        pipelined: false,
    }
    .run::<Reporter, _, _>(move |state| {
        let args = args.clone();
        let config_root = config_root.clone();
        Box::pin(async move {
            ensure_modules_dir_within_config_root(&state.config.modules_dir, &config_root)?;
            args.run::<Reporter>(state, None).await
        })
    })
    .await
}

fn ensure_modules_dir_within_config_root(
    modules_dir: &Path,
    config_root: &Path,
) -> miette::Result<()> {
    let normalized_root = pnpm_fs::lexical_normalize(config_root);
    let resolved_root = pnpm_fs::realpath_missing(&normalized_root).unwrap_or(normalized_root);
    let normalized_modules = pnpm_fs::lexical_normalize(modules_dir);
    let within_root = pnpm_fs::realpath_missing(&normalized_modules)
        .is_ok_and(|resolved_modules| resolved_modules.starts_with(&resolved_root))
        && pnpm_fs::realpath_missing(modules_dir)
            .unwrap_or(normalized_modules)
            .starts_with(&resolved_root);
    if !within_root {
        let modules_dir = modules_dir.display();
        let cr = config_root.display();
        return Err(miette::miette!(
            "refusing prune: modules_dir ({modules_dir}) is outside workspace root ({cr})",
        ));
    }
    Ok(())
}

async fn run_dedicated_dedupe<Reporter: self::Reporter + 'static>(
    args: DedupeArgs,
    cfg: &'static Config,
    projects: DedicatedProjects,
    lockfile_path: &Path,
    existing: Option<String>,
    guard: Option<dedupe::LockfileGuard>,
) -> miette::Result<()> {
    DedicatedProjectRuns {
        config: cfg,
        projects,
        require_lockfile: false,
        http_client: Some(State::new_http_client(cfg)?),
        prune_excludes: !args.check,
        sync_injected_deps: false,
        pipelined: false,
    }
    .run::<Reporter, _, _>(|state| {
        let args = args.clone();
        let lockfile_path = lockfile_path.to_path_buf();
        let existing = existing.clone();
        Box::pin(async move {
            dedupe_dedicated_project::<Reporter>(args, state, &lockfile_path, existing.as_deref())
                .await
        })
    })
    .await?;
    if let Some(guard) = guard {
        dedupe::check_lockfile::<Reporter>(existing.as_deref(), guard, lockfile_path)?;
    }
    Ok(())
}
