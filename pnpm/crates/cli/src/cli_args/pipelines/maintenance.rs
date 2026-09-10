use super::{
    Config, Context, DedicatedProjectRuns, DedicatedProjects, DedupeArgs, InstallFamilyPlan, Path,
    PathBuf, PruneArgs, Reporter, State, config_deps, dedupe, resolve_bool_override,
    select_install_family_plan,
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

        config_deps::prepare::<Reporter>(self.cfg, &self.config_root, false).await?;
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
                    projects,
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
/// `virtual_store_dir`), and the CLI `--ignore-scripts` flag must win
/// over any hook-set value.
pub(crate) struct PrunePipeline {
    pub(crate) args: PruneArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) manifest_path: PathBuf,
}

impl PrunePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let PrunePipeline { args, cfg, config_root, manifest_path } = self;

        config_deps::prepare::<Reporter>(cfg, &config_root, false).await?;
        // Validate path containment AFTER hooks: updateConfig can mutate
        // modules_dir / virtual_store_dir via WorkspaceSettings::apply_to,
        // so the check must use the final (post-hook) config values.
        // The install pipeline's prune_target_within_modules also validates
        // VSD containment, but only at sweep time; this earlier check
        // catches a misconfigured modules_dir itself (e.g. an absolute
        // path outside the workspace) before any destructive work begins.
        //
        // `config_root` is `cfg.workspace_dir` when present, or the
        // canonicalized `--dir` otherwise — a meaningful containment
        // boundary in both cases.
        if !cfg.modules_dir.starts_with(&config_root) {
            let modules_dir = cfg.modules_dir.display();
            let cr = config_root.display();
            return Err(miette::miette!(
                "refusing prune: modules_dir ({modules_dir}) is outside workspace root ({cr})",
            ));
        }
        // Apply prune-specific overrides after hooks so that:
        // - `modules_cache_max_age = 0` forces the virtual-store sweep
        //   on the final (post-hook) config paths.
        // - `--ignore-scripts` from the CLI wins over any value the
        //   hooks set via `WorkspaceSettings::apply_to`.
        cfg.modules_cache_max_age = 0;
        cfg.ignore_scripts =
            resolve_bool_override(args.ignore_scripts, args.no_ignore_scripts, cfg.ignore_scripts);
        let cfg: &'static Config = cfg;
        let state = State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
        Box::pin(args.run::<Reporter>(state)).await
    }
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
    }
    .run(|state| {
        Box::pin(dedupe_dedicated_project::<Reporter>(
            args.clone(),
            state,
            lockfile_path,
            existing.as_deref(),
        ))
    })
    .await?;
    if let Some(guard) = guard {
        dedupe::check_lockfile::<Reporter>(existing.as_deref(), guard, lockfile_path)?;
    }
    Ok(())
}
