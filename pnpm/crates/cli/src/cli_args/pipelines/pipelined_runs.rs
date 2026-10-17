//! Running the per-project installs of a workspace whose projects keep their
//! own lockfiles ahead of the workspace projects they depend on.
//!
//! Resolving, fetching and materializing a project reads nothing the installs
//! of its workspace dependencies produce, so a project starts once each of
//! them has started, with at most `workspaceConcurrency` installs in flight.
//! An install counts from its start until it returns, waits included, because
//! it retains materialized state. Every install in flight then depends only on
//! installs that are in flight or done, so the earliest of them can finish.
//! A project waits for its workspace dependencies' installs
//! only where they start to matter: before it links its dependencies and runs
//! its lifecycle scripts. The scripts of a project therefore still run after
//! the scripts of every workspace project it depends on, as they do when each
//! install waits for its dependencies before it starts.

use super::{
    Arc, Config, DedicatedProjectRuns, HashMap, HashSet, Path, PathBuf, State,
    dedicated_runs::record_dedicated_result, sequence_project_dependencies,
};
use crate::state::DedicatedCaches;
use futures_util::{
    FutureExt,
    future::BoxFuture,
    stream::{FuturesUnordered, StreamExt},
};
use pnpm_package_manager::WorkspaceDependenciesInstalled;
use std::{
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Semaphore, watch};

/// How far a project's install got, for the projects that depend on it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Installed {
    Pending,
    /// Holds one of the in-flight slots.
    Started,
    Succeeded,
    Failed,
}

/// The projects whose install may run ahead of the workspace projects they
/// depend on. A project whose `preinstall` or `pnpm:devPreinstall` script
/// runs before its install reaches the wait, or that copies a workspace
/// project into its virtual store (an injected or `file:` dependency) while
/// materializing, waits for them before it starts.
pub(in crate::cli_args::pipelines) fn early_starts(
    config: &Config,
    projects: &[pnpm_workspace::Project],
) -> HashSet<PathBuf> {
    if config.inject_workspace_packages {
        return HashSet::new();
    }
    projects
        .iter()
        .filter(|project| can_start_early(project.manifest.value()))
        .map(|project| project.root_dir.clone())
        .collect()
}

fn can_start_early(manifest: &serde_json::Value) -> bool {
    let has_preinstall = ["preinstall", "pnpm:devPreinstall"]
        .into_iter()
        .any(|script| {
            manifest
                .get("scripts")
                .and_then(|scripts| scripts.get(script))
                .is_some()
        });
    let injects = manifest
        .get("dependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|meta| {
            meta.values()
                .any(|entry| entry.get("injected") == Some(&serde_json::Value::Bool(true)))
        });
    let copies_a_directory = ["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .filter_map(|group| manifest.get(group)?.as_object())
        .flat_map(serde_json::Map::values)
        .any(|spec| {
            spec.as_str()
                .is_some_and(|spec| spec.starts_with("file:"))
        });
    !(has_preinstall || injects || copies_a_directory)
}

impl DedicatedProjectRuns<'_> {
    pub(super) async fn run_projects_pipelined<Runner, RunFuture>(
        &self,
        run: Runner,
    ) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send + 'static,
    {
        let order = sequence_project_dependencies(&self.projects.dependencies);
        let concurrency = usize::try_from(self.config.workspace_concurrency)
            .unwrap_or(usize::MAX)
            .clamp(1, Semaphore::MAX_PERMITS);
        let run = PipelinedRun {
            runs: self,
            states: order
                .iter()
                .map(|_| watch::channel(Installed::Pending).0)
                .collect(),
            slots: Semaphore::new(concurrency),
            stopped: Arc::new(AtomicBool::new(false)),
            first_error: Mutex::new(None),
            caches: DedicatedCaches::default(),
            run: &run,
        };
        let position: HashMap<&Path, usize> = order
            .iter()
            .enumerate()
            .map(|(index, project_dir)| (project_dir.as_path(), index))
            .collect();
        order
            .iter()
            .enumerate()
            .map(|(index, project_dir)| run.project(index, project_dir, &position))
            .collect::<FuturesUnordered<_>>()
            .collect::<()>()
            .await;
        run.first_error
            .into_inner()
            .expect("dedicated install error lock is not poisoned")
            .map_or(Ok(()), Err)
    }
}

struct PipelinedRun<'r, 'c, Runner> {
    runs: &'r DedicatedProjectRuns<'c>,
    /// Indexed like the dependency order the projects start in.
    states: Vec<watch::Sender<Installed>>,
    slots: Semaphore,
    /// Set by the first failure when the command stops at it, so that no
    /// project that has yet to start, or to link, does.
    stopped: Arc<AtomicBool>,
    first_error: Mutex<Option<miette::Report>>,
    caches: DedicatedCaches,
    run: &'r Runner,
}

impl<Runner, RunFuture> PipelinedRun<'_, '_, Runner>
where
    Runner: Fn(State) -> RunFuture + Sync,
    RunFuture: Future<Output = miette::Result<()>> + Send + 'static,
{
    async fn project(&self, index: usize, project_dir: &Path, position: &HashMap<&Path, usize>) {
        let dependencies_installed = self.dependencies_installed(index, project_dir, position);
        let starts_early = self.runs.projects.early_starts.contains(project_dir);
        if starts_early {
            self.dependencies_started(index, project_dir, position).await;
        } else if !dependencies_installed.clone().await {
            self.states[index].send_replace(Installed::Failed);
            return;
        }
        let slot = self.slots.acquire().await.expect("the install slots are never closed");
        if self.stopped.load(Ordering::Acquire) {
            self.states[index].send_replace(Installed::Failed);
            return;
        }
        self.states[index].send_replace(Installed::Started);
        let gate = starts_early.then(|| self.gate(dependencies_installed.clone()));
        let result = self.runs.install_project(project_dir, &self.caches, gate, self.run).await;
        drop(slot);
        let failed = result.is_err();
        record_dedicated_result(&self.first_error, result);
        if failed && self.runs.config.bail {
            self.stopped.store(true, Ordering::Release);
        }
        // An install that had nothing to link returns before it waits, and
        // the projects that depend on this one still rely on its own
        // dependencies being installed once it is.
        let dependencies_succeeded = dependencies_installed.await;
        self.states[index].send_replace(if failed || !dependencies_succeeded {
            Installed::Failed
        } else {
            Installed::Succeeded
        });
    }

    /// Resolves once every workspace project `project_dir` depends on has
    /// settled. Only projects that start earlier count, which breaks a cycle
    /// the way the dependency order does. `false` when one of them failed
    /// and the command stops at the first failure.
    fn dependencies_installed(
        &self,
        index: usize,
        project_dir: &Path,
        position: &HashMap<&Path, usize>,
    ) -> WorkspaceDependenciesInstalled {
        let dependencies = self.dependency_states(index, project_dir, position);
        let bail = self.runs.config.bail;
        let settled: BoxFuture<'static, bool> = async move {
            let mut all_succeeded = true;
            for mut dependency in dependencies {
                let installed = dependency
                    .wait_for(|installed| {
                        matches!(installed, Installed::Succeeded | Installed::Failed)
                    })
                    .await
                    .map_or(Installed::Failed, |installed| *installed);
                all_succeeded &= installed == Installed::Succeeded;
            }
            all_succeeded || !bail
        }
        .boxed();
        settled.shared()
    }

    /// Resolves once every workspace project `project_dir` depends on, as
    /// [`Self::dependencies_installed`] counts them, has started or settled.
    async fn dependencies_started(
        &self,
        index: usize,
        project_dir: &Path,
        position: &HashMap<&Path, usize>,
    ) {
        for mut dependency in self.dependency_states(index, project_dir, position) {
            // A closed channel means the dependency's install is gone, which
            // is as good as settled for starting.
            let _ = dependency.wait_for(|installed| *installed != Installed::Pending).await;
        }
    }

    fn dependency_states(
        &self,
        index: usize,
        project_dir: &Path,
        position: &HashMap<&Path, usize>,
    ) -> Vec<watch::Receiver<Installed>> {
        self.runs.projects.dependencies[project_dir]
            .iter()
            .filter_map(|dependency| position.get(dependency.as_path()).copied())
            .filter(|&dependency| dependency < index)
            .map(|dependency| self.states[dependency].subscribe())
            .collect()
    }

    /// `dependencies_installed` as the install awaits it: `false` too when
    /// the command stopped at another project's failure in the meantime, so
    /// the install links nothing.
    fn gate(
        &self,
        dependencies_installed: WorkspaceDependenciesInstalled,
    ) -> WorkspaceDependenciesInstalled {
        let stopped = Arc::clone(&self.stopped);
        let gate: BoxFuture<'static, bool> =
            async move { dependencies_installed.await && !stopped.load(Ordering::Acquire) }.boxed();
        gate.shared()
    }
}
