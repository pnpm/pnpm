//! Running the per-project installs of a workspace whose projects keep their
//! own lockfiles, each once the workspace projects it depends on are done.

use super::{
    Arc, Config, Context, DedicatedProjectRuns, DedicatedProjects, DedicatedSync, HashMap, HashSet,
    IndexMap, InstallArgs, Path, PathBuf, Reporter, State, ThrottledClient,
    ci_validation::PreparedDedicatedProject, dedicated_project_name, discover_workspace_projects,
    injected_source_dirs, pipelined_runs::early_starts, project_dependencies, project_names,
    prune_after_dedicated_installs, sync_dedicated_injected_deps,
};
use crate::{
    cli_args::recursive::{AutoExcludeRoot, select_recursive_projects},
    state::DedicatedCaches,
};
use pnpm_package_manager::WorkspaceDependenciesInstalled;
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, schedule_graph_async,
};
use std::{future::Future, sync::Mutex};

impl DedicatedProjectRuns<'_> {
    pub(super) async fn run<Rep, Runner, RunFuture>(self, run: Runner) -> miette::Result<()>
    where
        Rep: Reporter,
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send + 'static,
    {
        if self.pipelined {
            self.run_projects_pipelined(run).await?;
        } else {
            self.run_projects(run).await?;
        }
        if self.prune_excludes && self.projects.covers_workspace {
            let project_dirs: Vec<PathBuf> = self.projects.dependencies
                .keys()
                .cloned()
                .collect();
            pnpm_package_manager::verify_patches_against_project_lockfiles::<Rep>(
                self.config,
                &project_dirs,
            )
            .map_err(miette::Report::new)?;
            prune_after_dedicated_installs(self.config)?;
        }
        if self.sync_injected_deps {
            let project_dirs: Vec<PathBuf> = self.projects.dependencies
                .keys()
                .cloned()
                .collect();
            sync_dedicated_injected_deps(
                self.config,
                &project_dirs,
                &DedicatedSync {
                    names: &self.projects.names,
                    source_dirs: &self.projects.injected_source_dirs,
                },
            )?;
        }
        Ok(())
    }

    async fn run_projects<Runner, RunFuture>(&self, run: Runner) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send + 'static,
    {
        let first_error = Mutex::new(None);
        let caches = DedicatedCaches::default();
        let run_node = |project_dir: PathBuf| {
            let (first_error, caches, run) = (&first_error, &caches, &run);
            async move {
                let result = self.install_project(&project_dir, caches, None, run).await;
                record_dedicated_result(first_error, result)
            }
        };
        let on_node_skipped: fn(&PathBuf) = |_| {};
        schedule_graph_async(
            &self.projects.dependencies,
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

    /// Install the project at `project_dir` through `run`, sharing `caches`
    /// with the other projects and waiting for `dependencies_installed`.
    pub(super) async fn install_project<Runner, RunFuture>(
        &self,
        project_dir: &Path,
        caches: &DedicatedCaches,
        dependencies_installed: Option<WorkspaceDependenciesInstalled>,
        run: &Runner,
    ) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send + 'static,
    {
        let state = init_dedicated_project_state(
            self.config,
            project_dir,
            self.projects.names.get(project_dir).map(String::as_str),
            self.require_lockfile,
            self.http_client.as_ref().map(Arc::clone),
        )?
        .into_dedicated_project(
            caches,
            dependencies_installed,
            self.pipelined.then(|| self.projects.workspace.clone()),
        );
        // A project's install blocks its thread in places, such as while its
        // lifecycle scripts run. On its own task, the other projects'
        // installs move to another worker then.
        tokio::spawn(run(state)).await
            .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
    }
}

/// Build the project-anchored `State` for one project of a
/// `sharedWorkspaceLockfile: false` workspace: clone `cfg`, re-anchor its
/// output paths and per-project settings under `project_dir` via
/// [`Config::anchor_dedicated_project`], and initialize the state. The
/// clone is leaked because [`State::init`] needs a `&'static Config`. The
/// leak is bounded by the project count, happens once per CLI invocation,
/// and is reclaimed at process exit, the same lifetime deploy's derived
/// install config has.
pub(super) fn init_dedicated_project_state(
    cfg: &Config,
    project_dir: &Path,
    project_name: Option<&str>,
    require_lockfile: bool,
    http_client: Option<Arc<ThrottledClient>>,
) -> miette::Result<State> {
    let mut project_config = cfg.clone();
    project_config.anchor_dedicated_project(project_dir, project_name);
    let project_config = Config::leak(project_config);
    let manifest_path = project_dir.join("package.json");
    match http_client {
        Some(http_client) => {
            let lockfile = State::lazy_lockfile(project_config, &manifest_path, require_lockfile);
            State::init_with_lockfile_and_http_client(
                manifest_path,
                project_config,
                lockfile,
                http_client,
            )
        }
        None => State::init(manifest_path, project_config, require_lockfile),
    }
    .wrap_err_with(|| format!("initialize the state for {}", project_dir.display()))
}

pub(super) fn record_dedicated_result(
    first_error: &Mutex<Option<miette::Report>>,
    result: miette::Result<()>,
) -> TaskCompletion {
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

pub(super) async fn run_dedicated_projects<Reporter: self::Reporter + 'static>(
    args: &InstallArgs,
    cfg: &Config,
    projects: DedicatedProjects,
    require_lockfile: bool,
    http_client: Arc<ThrottledClient>,
    prepared_by_dir: Arc<Mutex<HashMap<PathBuf, PreparedDedicatedProject>>>,
) -> miette::Result<()> {
    DedicatedProjectRuns {
        config: cfg,
        projects,
        require_lockfile,
        http_client: Some(http_client),
        prune_excludes: !args.materialization.dry_run,
        sync_injected_deps: !(args.lockfile.only || args.materialization.dry_run),
        pipelined: true,
    }
    .run::<Reporter, _, _>(|mut state| {
        let prepared = state.manifest
            .path()
            .parent()
            .and_then(|dir| {
                prepared_by_dir
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(dir)
            })
            .map(|entry| {
                state.lockfile = entry.lockfile;
                entry.install
            });
        Box::pin(args.clone().run_prepared::<Reporter>(state, prepared))
    })
    .await
}

/// `sharedWorkspaceLockfile: false` workspace install: one independent
/// single-project install per workspace project — each gets its own
/// `pnpm-lock.yaml`, `node_modules`, and virtual store, mirroring
/// pnpm's dedicated-lockfile per-project loop in its recursive
/// dispatch. The workspace root participates when it has a manifest,
/// matching the project set a shared-lockfile workspace install covers.
/// Projects whose workspace dependencies have finished run concurrently
/// under `workspaceConcurrency`, as a recursive install does.
pub(in crate::cli_args) async fn run_dedicated_lockfile_workspace_install<
    Reporter: self::Reporter + 'static,
>(
    args: &InstallArgs,
    cfg: &Config,
    workspace_root: &Path,
    require_lockfile: bool,
    http_client: Arc<ThrottledClient>,
) -> miette::Result<()> {
    let projects = discover_dedicated_workspace_projects(cfg, workspace_root)?;
    run_dedicated_projects::<Reporter>(
        args,
        cfg,
        projects,
        require_lockfile,
        http_client,
        Arc::default(),
    )
    .await
}

pub(super) fn discover_dedicated_workspace_projects(
    cfg: &Config,
    workspace_root: &Path,
) -> miette::Result<DedicatedProjects> {
    let (projects, _patterns) = discover_workspace_projects(workspace_root, cfg)?;
    let mut names = project_names(cfg, &projects);
    let normalized_root = pnpm_fs::lexical_normalize(workspace_root);
    let mut dependencies = IndexMap::with_capacity(projects.len() + 1);
    let mut other_dirs = Vec::new();
    if pnpm_package_manifest::project_manifest_path(workspace_root).is_file()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_root)
    {
        dependencies.insert(workspace_root.to_path_buf(), Vec::new());
        other_dirs.push(workspace_root.to_path_buf());
        // The root is installed alongside the discovered projects but is
        // not one of them, so its name is not in the map yet.
        if let Some(name) = dedicated_project_name(cfg, workspace_root) {
            names.insert(workspace_root.to_path_buf(), name);
        }
    }
    let injected_source_dirs = dedicated_injected_source_dirs(&projects, &other_dirs)?;
    let selection =
        select_recursive_projects(&projects, cfg, workspace_root, AutoExcludeRoot::Disabled)?;
    dependencies.extend(project_dependencies(&selection, cfg.sort));
    Ok(DedicatedProjects {
        dependencies,
        names,
        covers_workspace: true,
        injected_source_dirs,
        early_starts: early_starts(cfg, &projects),
        workspace: pnpm_package_manager::DiscoveredWorkspace::new(projects),
    })
}

/// See [`injected_source_dirs`]. `other_dirs` are the directories installed
/// alongside `projects` without being discovered, such as the workspace
/// root. Their manifests are read here, before any lifecycle script runs.
fn dedicated_injected_source_dirs(
    projects: &[pnpm_workspace::Project],
    other_dirs: &[PathBuf],
) -> miette::Result<HashSet<PathBuf>> {
    let other_manifests = other_dirs
        .iter()
        .map(|dir| {
            pnpm_package_manifest::safe_read_project_manifest_from_dir(dir)
                .map_err(miette::Report::new)
        })
        .collect::<miette::Result<Vec<_>>>()?;
    Ok(injected_source_dirs(
        projects
            .iter()
            .map(|project| (project.root_dir.as_path(), Some(project.manifest.value())))
            .chain(
                other_dirs
                    .iter()
                    .map(PathBuf::as_path)
                    .zip(other_manifests.iter().map(Option::as_ref)),
            ),
    ))
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod install_tests;
