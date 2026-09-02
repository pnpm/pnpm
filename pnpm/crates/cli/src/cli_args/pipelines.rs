use super::{
    add::AddArgs,
    dedupe::{self, DedupeArgs},
    deploy::DeployArgs,
    install::{InstallArgs, resolve_bool_override},
    package_manager::{PackageManagerToSync, package_manager_to_sync, read_manifest_json},
    prune::PruneArgs,
    recursive::{
        AutoExcludeRoot, discover_workspace_projects, filtered_projects_dependencies,
        select_recursive_projects,
    },
    remove::RemoveArgs,
    update::UpdateArgs,
    update_changeset::UpdateChangesetContext,
};
use crate::{
    State,
    cli_args::{
        config_warnings::warn_unmatched_registry_options,
        legacy_pnpm_field::warn_ignored_pnpm_manifest_fields,
        override_version_references::warn_deprecated_override_version_references,
        reporter::{ReporterType, reporter_emit},
    },
    config_deps,
};
use indexmap::IndexMap;
use miette::Context;
use pnpm_config::{Config, Host};
use pnpm_package_manager::{PathNode, graph_sequencer};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, ScopeLog};
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, schedule_graph_async,
};
use std::{
    collections::{BTreeMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct InstallFamilySelection {
    pub(crate) workspace_root: PathBuf,
    pub(crate) projects: Vec<pnpm_workspace::Project>,
    pub(crate) project_dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
    pub(crate) ordered_dirs: Vec<PathBuf>,
    pub(crate) selected_dirs: Arc<HashSet<PathBuf>>,
    pub(crate) install_dirs: Arc<HashSet<PathBuf>>,
    pub(crate) active_manifest_is_standin: bool,
    /// `Some` when the plan already ran the install's cycle search over
    /// the same graph the install would rebuild (the unnarrowed case);
    /// empty means the projects are orderable. `None` — the install
    /// searches itself.
    pub(crate) workspace_cycles: Option<Vec<Vec<PathBuf>>>,
}

/// How a recursive / filtered install-family command should be dispatched,
/// resolved from the config and the workspace selection.
pub(crate) enum InstallFamilyPlan {
    /// Not recursive (`!cfg.recursive`): run against the active project only.
    /// The pipelines keep their own non-recursive handling (the dedicated
    /// per-project anchor for `add` / `update` / `remove`, and the
    /// dedicated-lockfile workspace install for `install`).
    Single,
    /// Recursive / filtered over a shared workspace lockfile: one mutation
    /// pass writes every selected importer into the shared `pnpm-lock.yaml`.
    Shared(Box<InstallFamilySelection>),
    /// Recursive / filtered with one lockfile per project
    /// (`sharedWorkspaceLockfile: false`): the selected project directories,
    /// each installed independently against its own `pnpm-lock.yaml`,
    /// `node_modules`, and virtual store. Dependency-ready projects run under
    /// the workspace-concurrency limit.
    PerProject(IndexMap<PathBuf, Vec<PathBuf>>),
}

struct DedicatedProjectRuns<'a> {
    config: &'a Config,
    project_dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
    require_lockfile: bool,
}

impl DedicatedProjectRuns<'_> {
    async fn run<Runner, RunFuture>(self, run: Runner) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send,
    {
        let first_error: std::sync::Mutex<Option<miette::Report>> = std::sync::Mutex::new(None);
        let config = self.config;
        let require_lockfile = self.require_lockfile;
        let run = &run;
        let run_node = |project_dir: PathBuf| {
            let first_error = &first_error;
            async move {
                let result =
                    match init_dedicated_project_state(config, &project_dir, require_lockfile) {
                        Ok(state) => run(state).await,
                        Err(error) => Err(error),
                    };
                match result {
                    Ok(()) => TaskCompletion::Passed,
                    Err(error) => {
                        first_error
                            .lock()
                            .expect("dedicated install error lock is not poisoned")
                            .get_or_insert(error);
                        TaskCompletion::Failed
                    }
                }
            }
        };
        let on_node_skipped: fn(&PathBuf) = |_| {};
        schedule_graph_async(
            &self.project_dependencies,
            &ScheduleGraphAsyncOptions::new(
                usize::try_from(self.config.workspace_concurrency).unwrap_or(usize::MAX).max(1),
                self.config.bail,
                &run_node,
                &on_node_skipped,
            )
            .continue_on_failure(!self.config.bail),
        )
        .await;
        first_error
            .into_inner()
            .expect("dedicated install error lock is not poisoned")
            .map_or(Ok(()), Err)
    }
}

fn select_install_family_plan<Reporter: self::Reporter>(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
    precompute_workspace_cycles: bool,
) -> miette::Result<InstallFamilyPlan> {
    let Some(selection) = select_workspace_projects_with_cycles(
        cfg,
        prefix,
        manifest_path,
        recursive_sort,
        auto_exclude_root,
        precompute_workspace_cycles,
    )?
    else {
        return Ok(InstallFamilyPlan::Single);
    };
    // Report what the `--filter` / `-r` selection resolved to, so the user
    // can confirm it before the install acts on it. Emitted once here for
    // every plan shape below — a `PerProject` plan installs each selected
    // project separately, and those child installs must not each report
    // the workspace again. The unnarrowed install reports its own scope
    // from inside the installer, where the workspace walk it already does
    // supplies the count.
    Reporter::emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected: selection.selected_dirs.len(),
        total: Some(selection.projects.len()),
        workspace_prefix: Some(selection.workspace_root.to_string_lossy().into_owned()),
    }));
    if !cfg.shares_one_lockfile() {
        return Ok(InstallFamilyPlan::PerProject(selection.project_dependencies));
    }
    Ok(InstallFamilyPlan::Shared(Box::new(selection)))
}

pub(crate) fn select_workspace_projects(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
) -> miette::Result<Option<InstallFamilySelection>> {
    select_workspace_projects_with_cycles(
        cfg,
        prefix,
        manifest_path,
        recursive_sort,
        auto_exclude_root,
        false,
    )
}

/// [`select_workspace_projects`], optionally running the install's
/// workspace-cycle search over the selection graph while it is still in
/// hand. Callers pass `true` only for a run that is certain to reach
/// the cycle check — the "Already up to date" fast path returns before
/// it, and a search it never reads would tax exactly that path.
fn select_workspace_projects_with_cycles(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
    precompute_workspace_cycles: bool,
) -> miette::Result<Option<InstallFamilySelection>> {
    if !cfg.recursive {
        return Ok(None);
    }

    let workspace_root = cfg.workspace_dir.as_deref().unwrap_or(prefix).to_path_buf();
    let (mut projects, workspace_patterns) = discover_workspace_projects(&workspace_root, cfg)?;
    if let Some(runtime_on_fail) = cfg.runtime_on_fail {
        for project in &mut projects {
            pnpm_package_manifest::apply_runtime_on_fail_override(
                project.manifest.value_mut(),
                runtime_on_fail.as_str(),
            );
        }
    }
    let (project_dependencies, ordered_dirs, selected_dirs, workspace_cycles) = {
        let selection = select_recursive_projects(
            &projects,
            cfg,
            prefix,
            if auto_exclude_root {
                AutoExcludeRoot::Enabled { workspace_patterns: workspace_patterns.as_deref() }
            } else {
                AutoExcludeRoot::Disabled
            },
        )?;
        // Computed here only when it answers exactly what the install's
        // own cycle search over its rebuilt graph would: an unnarrowed
        // selection (`all` unset) is the whole graph in build order, so
        // running the same search over it here lets the install skip
        // the rebuild. A `--filter` / `--filter-prod` selection reorders
        // the nodes (and prod-prunes some edges), so the install keeps
        // its own search there.
        let workspace_cycles = (precompute_workspace_cycles
            && selection.all.is_none()
            && !cfg.ignore_workspace_cycles)
            .then(|| {
                pnpm_package_manager::workspace_cycles(&selection.selected).unwrap_or_default()
            });
        let project_dependencies = if recursive_sort {
            filtered_projects_dependencies(
                &selection.selected,
                selection.full_graph(),
                selection.prod_all.as_ref(),
                &selection.prod_only_selected,
            )
        } else {
            let mut dirs = selection.selected.keys().cloned().collect::<Vec<_>>();
            dirs.sort();
            dirs.into_iter().map(|dir| (dir, Vec::new())).collect()
        };
        // Sequenced over borrowed paths: cloning a workspace-scale edge
        // map just to sort it cost more than the sort.
        let ordered_dirs = graph_sequencer(
            &project_dependencies
                .iter()
                .map(|(key, value)| {
                    (
                        PathNode(key.as_path()),
                        value.iter().map(|dir| PathNode(dir)).collect::<Vec<_>>(),
                    )
                })
                .collect(),
            &project_dependencies.keys().map(|dir| PathNode(dir)).collect::<Vec<_>>(),
        )
        .order
        .into_iter()
        .map(|node| node.0.to_path_buf())
        .collect();
        let selected_dirs: Arc<HashSet<PathBuf>> =
            Arc::new(selection.selected.keys().cloned().collect());
        (project_dependencies, ordered_dirs, selected_dirs, workspace_cycles)
    };

    let active_dir = manifest_path.parent().expect("manifest path always has a parent dir");
    let normalized_active_dir = pnpm_fs::lexical_normalize(active_dir);
    let active_manifest_is_standin = !active_dir.join("package.json").is_file()
        && pnpm_workspace::try_read_project_manifest(active_dir)
            .map_err(miette::Report::new)?
            .is_none()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_active_dir);
    let normalized_workspace_root = pnpm_fs::lexical_normalize(&workspace_root);
    let mut install_dirs = selected_dirs.as_ref().clone();
    if let Some(workspace_root_project) = projects
        .iter()
        .find(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_workspace_root)
    {
        install_dirs.insert(workspace_root_project.root_dir.clone());
    }

    Ok(Some(InstallFamilySelection {
        workspace_root,
        projects,
        project_dependencies,
        ordered_dirs,
        selected_dirs,
        install_dirs: Arc::new(install_dirs),
        active_manifest_is_standin,
        workspace_cycles,
    }))
}

/// Build the project-anchored `State` for one project of a
/// `sharedWorkspaceLockfile: false` workspace: clone `cfg`, re-anchor its
/// output paths under `project_dir` via [`Config::anchor_lockfile_paths`],
/// and initialize the state. The clone is leaked because [`State::init`] needs
/// a `&'static Config`; see [`run_dedicated_lockfile_workspace_install`] for
/// why the bounded leak is acceptable.
fn init_dedicated_project_state(
    cfg: &Config,
    project_dir: &Path,
    require_lockfile: bool,
) -> miette::Result<State> {
    let mut project_config = cfg.clone();
    project_config.anchor_lockfile_paths(project_dir);
    let project_config = Config::leak(project_config);
    State::init(project_dir.join("package.json"), project_config, require_lockfile)
        .wrap_err_with(|| format!("initialize the state for {}", project_dir.display()))
}

/// The reporter-generic body of `pacquet install`: it threads one `Reporter`
/// type through config-dependency sync, the `updateConfig` hooks, and the
/// install itself. Lifting it out of the dispatch keeps the three
/// `ReporterType` arms to a single line each.
pub(crate) struct InstallPipeline {
    pub(crate) args: InstallArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    pub(crate) require_lockfile: bool,
    pub(crate) frozen_lockfile: bool,
}

impl InstallPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let InstallPipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
            require_lockfile,
            frozen_lockfile,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                frozen_lockfile,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, frozen_lockfile).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
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
        let lockfile = cfg
            .shares_one_lockfile()
            .then(|| State::lazy_lockfile(cfg, &manifest_path, require_lockfile));
        let certain_full_install = cfg.shares_one_lockfile() && {
            let manifest_dir =
                manifest_path.parent().expect("manifest path always has a parent dir");
            let lockfile_dir = cfg.lockfile_dir_for(manifest_dir);
            frozen_lockfile
                || cfg.force
                || !pnpm_workspace_state::get_file_path(lockfile_dir).is_file()
        };
        if let Some(lockfile) = lockfile.as_ref()
            && !args.fix_lockfile
            && certain_full_install
        {
            lockfile.prefetch();
        }
        let plan = select_install_family_plan::<Reporter>(
            cfg,
            &prefix,
            &manifest_path,
            recursive_sort,
            false,
            certain_full_install,
        )?;
        match plan {
            InstallFamilyPlan::PerProject(project_dependencies) => {
                DedicatedProjectRuns { config: cfg, project_dependencies, require_lockfile }
                    .run(|state| Box::pin(args.clone().run::<Reporter>(state)))
                    .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = cfg;
                let state = init_shared_state(manifest_path, cfg, require_lockfile, lockfile)?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                if !cfg.shares_one_lockfile()
                    && let Some(workspace_dir) = cfg.workspace_dir.clone()
                {
                    let cfg: &'static Config = cfg;
                    return run_dedicated_lockfile_workspace_install::<Reporter>(
                        &args,
                        cfg,
                        &workspace_dir,
                        require_lockfile,
                    )
                    .await;
                }
                let cfg: &'static Config = cfg;
                let state = init_shared_state(manifest_path, cfg, require_lockfile, lockfile)?;
                Box::pin(args.run::<Reporter>(state)).await
            }
        }
    }
}

/// [`State::init`], consuming the pipeline's pre-built lockfile when
/// there is one so an already-running prefetch isn't thrown away.
fn init_shared_state(
    manifest_path: PathBuf,
    config: &'static Config,
    require_lockfile: bool,
    lockfile: Option<pnpm_lockfile::LazyLockfile>,
) -> miette::Result<State> {
    match lockfile {
        Some(lockfile) => State::init_with_lockfile(manifest_path, config, lockfile),
        None => State::init(manifest_path, config, require_lockfile),
    }
    .wrap_err("initialize the state")
}

pub(crate) struct AddPipeline {
    pub(crate) args: AddArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    /// [`AddArgs::parse_config_dependencies`]'s output, parsed by the dispatch
    /// before this pipeline scaffolds a manifest. `Some` exactly when
    /// `--config` was passed.
    pub(crate) config_dependencies: Option<BTreeMap<String, String>>,
}

impl AddPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let AddPipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
            config_dependencies,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        // `--config` targets the workspace's configuration dependencies, not
        // any project's manifest, so it bypasses project selection entirely.
        let plan = if config_dependencies.is_some() {
            InstallFamilyPlan::Single
        } else {
            select_install_family_plan::<Reporter>(
                cfg,
                &prefix,
                &manifest_path,
                recursive_sort,
                true,
                false,
            )?
        };
        match plan {
            InstallFamilyPlan::PerProject(project_dependencies) => {
                // Dedicated per-project lockfiles: add the packages to each
                // selected project independently.
                DedicatedProjectRuns { config: cfg, project_dependencies, require_lockfile: false }
                    .run(|state| Box::pin(args.clone().run::<Reporter>(state, None)))
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
                // Dedicated per-project lockfiles: `add` mutates only the
                // active project, whose outputs anchor at the project dir.
                // `--config` targets the workspace's configuration
                // dependencies, which stay workspace-anchored.
                if config_dependencies.is_none()
                    && !cfg.shares_one_lockfile()
                    && cfg.workspace_dir.is_some()
                {
                    let manifest_dir = manifest_path
                        .parent()
                        .expect("manifest path always has a parent dir")
                        .to_path_buf();
                    cfg.anchor_lockfile_paths(&manifest_dir);
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state, config_dependencies)).await
            }
        }
    }
}

pub(crate) struct UpdatePipeline {
    pub(crate) args: UpdateArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl UpdatePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let UpdatePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let plan = select_install_family_plan::<Reporter>(
            cfg,
            &prefix,
            &manifest_path,
            recursive_sort,
            false,
            false,
        )?;
        // An empty selection has nothing to update, and — like the shared
        // path — must not generate a changeset.
        match &plan {
            InstallFamilyPlan::PerProject(project_dependencies)
                if project_dependencies.is_empty() =>
            {
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
            && !cfg.shares_one_lockfile()
            && cfg.workspace_dir.is_some()
        {
            let manifest_dir = manifest_path
                .parent()
                .expect("manifest path always has a parent dir")
                .to_path_buf();
            cfg.anchor_lockfile_paths(&manifest_dir);
        }
        let generate_changeset = if args.changeset {
            true
        } else if args.no_changeset {
            false
        } else {
            cfg.update_config.changeset.unwrap_or(false)
        };
        let changeset_context = generate_changeset
            .then(|| UpdateChangesetContext::capture(cfg, &manifest_path))
            .transpose()?;
        match plan {
            InstallFamilyPlan::PerProject(project_dependencies) => {
                DedicatedProjectRuns { config: cfg, project_dependencies, require_lockfile: false }
                    .run(|state| Box::pin(args.clone().run::<Reporter>(state)))
                    .await?;
            }
            InstallFamilyPlan::Shared(selection) => {
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await?;
            }
            InstallFamilyPlan::Single => {
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await?;
            }
        }
        if let Some(changeset_context) = changeset_context {
            changeset_context.generate::<Reporter>()?;
        }
        Ok(())
    }
}

pub(crate) struct RemovePipeline {
    pub(crate) args: RemoveArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl RemovePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let RemovePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let plan = select_install_family_plan::<Reporter>(
            cfg,
            &prefix,
            &manifest_path,
            recursive_sort,
            false,
            false,
        )?;
        match plan {
            InstallFamilyPlan::PerProject(project_dependencies) => {
                // Dedicated per-project lockfiles: remove the packages from
                // each selected project independently.
                DedicatedProjectRuns { config: cfg, project_dependencies, require_lockfile: false }
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
                    let manifest_dir = manifest_path
                        .parent()
                        .expect("manifest path always has a parent dir")
                        .to_path_buf();
                    cfg.anchor_lockfile_paths(&manifest_dir);
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
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
}

impl DeployPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir_ref: &Path,
    ) -> miette::Result<()> {
        let DeployPipeline { args, cfg, config_root, package_manager_to_sync } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let cfg: &'static Config = cfg;
        Box::pin(args.run::<Reporter>(cfg, dir_ref)).await
    }
}

/// `sharedWorkspaceLockfile: false` workspace install: one independent
/// single-project install per workspace project — each gets its own
/// `pnpm-lock.yaml`, `node_modules`, and virtual store, mirroring
/// pnpm's dedicated-lockfile per-project loop in its recursive
/// dispatch. The workspace root participates when it has a manifest,
/// matching the project set a shared-lockfile workspace install covers.
async fn run_dedicated_lockfile_workspace_install<Reporter: self::Reporter + 'static>(
    args: &super::install::InstallArgs,
    cfg: &Config,
    workspace_root: &Path,
    require_lockfile: bool,
) -> miette::Result<()> {
    let (projects, _patterns) = discover_workspace_projects(workspace_root, cfg)?;
    let normalized_root = pnpm_fs::lexical_normalize(workspace_root);
    let mut project_dirs: Vec<PathBuf> = Vec::with_capacity(projects.len() + 1);
    if workspace_root.join("package.json").is_file()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_root)
    {
        project_dirs.push(workspace_root.to_path_buf());
    }
    project_dirs.extend(projects.into_iter().map(|project| project.root_dir));
    // One `Config::leak` per project: `State::init` needs a
    // `&'static Config`, and a leaked shared reference can't be
    // reclaimed for the next iteration. The leak is bounded by the
    // project count, happens once per CLI invocation, and is
    // reclaimed at process exit — the same lifetime deploy's derived
    // install config has.
    for project_dir in project_dirs {
        let state = init_dedicated_project_state(cfg, &project_dir, require_lockfile)?;
        Box::pin(args.clone().run::<Reporter>(state)).await?;
    }
    Ok(())
}

/// Shared workspace-root and package-manager policy derivation used by the
/// install, dedupe, and prune dispatch paths.
pub(crate) fn derive_config_root_and_package_manager_to_sync(
    cfg: &Config,
    dir_ref: &Path,
    reporter: ReporterType,
) -> miette::Result<(PathBuf, Option<PackageManagerToSync>)> {
    let config_root = cfg.root_project_manifest_dir(dir_ref).to_path_buf();
    let root_manifest = read_manifest_json(&config_root.join("package.json"))
        .wrap_err("read package manager policy")?;
    // pnpm warns from config-reading, so the notice lands ahead of any
    // install output. This is the install family's earliest point that
    // knows the root manifest's directory.
    warn_ignored_pnpm_manifest_fields(root_manifest.as_ref());
    warn_deprecated_override_version_references(cfg, reporter_emit(reporter));
    warn_unmatched_registry_options(cfg);
    let package_manager_to_sync = root_manifest
        .as_ref()
        .and_then(|manifest| package_manager_to_sync(manifest, &config_root, cfg.pm_on_fail));
    Ok((config_root, package_manager_to_sync))
}

pub(crate) fn apply_install_cli_config(cfg: &mut Config, args: &InstallArgs) {
    cfg.offline = resolve_bool_override(args.offline, args.no_offline, cfg.offline);
    cfg.prefer_offline =
        resolve_bool_override(args.prefer_offline, args.no_prefer_offline, cfg.prefer_offline);
    cfg.frozen_store =
        resolve_bool_override(args.frozen_store, args.no_frozen_store, cfg.frozen_store);
    cfg.ignore_scripts =
        resolve_bool_override(args.ignore_scripts, args.no_ignore_scripts, cfg.ignore_scripts);
    cfg.ignore_pnpmfile = args.ignore_pnpmfile || cfg.ignore_pnpmfile;
    cfg.force = args.force || cfg.force;
    if let Some(network_concurrency) = args.network_concurrency {
        cfg.network_concurrency = network_concurrency;
    }
    if let Some(fetch_timeout) = args.fetch_timeout {
        cfg.fetch_timeout = fetch_timeout;
    }
    if let Some(fetch_warn_timeout_ms) = args.fetch_warn_timeout_ms {
        cfg.fetch_warn_timeout_ms = fetch_warn_timeout_ms;
    }
    if let Some(fetch_min_speed_ki_bps) = args.fetch_min_speed_ki_bps {
        cfg.fetch_min_speed_ki_bps = fetch_min_speed_ki_bps;
    }
    if let Some(user_agent) = args.user_agent.clone() {
        cfg.user_agent = user_agent;
    }
    if let Some(pnpr_server) = args.pnpr_server.clone() {
        cfg.pnpr_server = Some(pnpr_server);
    }
    // pnpm merges its CLI options into the config *before* deciding
    // `mergeGitBranchLockfiles`, so a pattern given on the command line
    // still gets matched against the current branch — and an explicit
    // `--merge-git-branch-lockfiles` settles the question without it.
    if args.merge_git_branch_lockfiles {
        cfg.merge_git_branch_lockfiles = true;
    } else if !args.merge_git_branch_lockfiles_branch_pattern.is_empty() {
        cfg.merge_git_branch_lockfiles_branch_pattern
            .clone_from(&args.merge_git_branch_lockfiles_branch_pattern);
        cfg.apply_git_branch_lockfile_derivation::<Host>();
    }
}

/// The reporter-generic body of `pacquet dedupe`: snapshots the lockfile
/// (when `--check`), runs config-dependency installation and `updateConfig`
/// hooks, then dispatches to the install pipeline. The snapshot wraps the
/// entire pipeline so any lockfile write made by config-deps is also covered
/// by the check gate.
pub(crate) struct DedupePipeline {
    pub(crate) args: DedupeArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) manifest_path: PathBuf,
}

impl DedupePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let DedupePipeline { args, cfg, config_root, package_manager_to_sync, manifest_path } =
            self;

        let lockfile_path = config_root.join(cfg.wanted_lockfile_name());

        // Snapshot before any config-dep writes so --check detects lockfile
        // changes made by config-dependency syncing as well.
        let existing =
            if args.check { dedupe::read_lockfile_snapshot(&lockfile_path)? } else { None };
        let guard =
            args.check.then(|| dedupe::LockfileGuard::new(existing.clone(), &lockfile_path));

        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let cfg: &'static Config = cfg;
        let state = State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
        Box::pin(args.run::<Reporter>(state, existing, guard, &lockfile_path)).await
    }
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
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) manifest_path: PathBuf,
}

impl PrunePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let PrunePipeline { args, cfg, config_root, package_manager_to_sync, manifest_path } = self;

        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
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
