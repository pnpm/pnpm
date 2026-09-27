//! Running the per-project installs of a workspace whose projects keep their
//! own lockfiles, each once the workspace projects it depends on are done.

use super::{
    Arc, Config, Context, DedicatedProjectRuns, DedicatedSync, Path, PathBuf, State,
    ThrottledClient, prune_after_dedicated_installs, sync_dedicated_injected_deps,
};
use crate::state::DedicatedCaches;
use pnpm_package_manager::WorkspaceDependenciesInstalled;
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, schedule_graph_async,
};
use std::{future::Future, sync::Mutex};

impl DedicatedProjectRuns<'_> {
    pub(super) async fn run<Runner, RunFuture>(self, run: Runner) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send + 'static,
    {
        if self.pipelined {
            self.run_projects_pipelined(run).await?;
        } else {
            self.run_projects(run).await?;
        }
        if self.prune_excludes && self.projects.covers_workspace {
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
        .into_dedicated_project(caches, dependencies_installed);
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
fn init_dedicated_project_state(
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
