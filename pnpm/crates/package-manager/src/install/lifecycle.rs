pub(in crate::install) use graph::{ProjectLifecycleGraph, project_lifecycle_graph};
pub(super) use uninstall::{project_script_stages, run_pre_uninstall_scripts};

mod graph;
mod project_script_runner;
mod uninstall;

use project_script_runner::{ProjectScriptRunner, run_project_stages};

use super::{
    Config, DEV_PREINSTALL_ALREADY_RAN_ENV, HashMap, InstallError, NodeLinker, Path, PathBuf,
    ROOT_PREINSTALL_ALREADY_RAN_ENV, Reporter, RunPostinstallHooks,
    project_requires_lifecycle_scripts,
};

use pnpm_executor::LifecycleScriptError;
use pnpm_workspace_task_scheduler::{ScheduleGraphOptions, TaskCompletion, schedule_graph};
use std::sync::Mutex;

/// Walk every workspace project's `package.json`. Returns `Ok(None)`
/// when no `pnpm-workspace.yaml` exists in (or above) `workspace_root`
/// — the install isn't a workspace install, so the caller should use
/// the top-level `Install.manifest` as its only importer and pass
/// `None` for the `workspace:`-spec lookup.
///
/// One walk feeds both [`super::build_workspace_packages_map`] (the npm
/// resolver's `workspace:` lookup) and the per-importer manifest list
/// the fresh-resolve path iterates over, so the manifests are read
/// from disk exactly once.
pub(super) fn load_workspace_projects(
    workspace_root: &std::path::Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    config: &Config,
) -> Result<Option<Vec<pnpm_workspace::Project>>, pnpm_workspace::FindWorkspaceProjectsError> {
    let Some(manifest) = workspace_manifest else { return Ok(None) };
    let opts = config.find_workspace_projects_opts(Some(
        pnpm_workspace::workspace_package_patterns(manifest),
    ));
    pnpm_workspace::find_workspace_projects(workspace_root, &opts).map(Some)
}

/// [`Config::extra_env_with_node_options`] plus the `NODE_OPTIONS` entry for
/// the selected project-level dependency loader. pnpm adds it only once it
/// links and builds, which is why `pnpm:devPreinstall` — running before the
/// file exists — takes the plain [`Config::extra_env_with_node_options`].
pub(super) fn project_lifecycle_extra_env(
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> HashMap<String, String> {
    let mut extra_env = config.extra_env_with_node_options();
    if matches!(node_linker, NodeLinker::Pnp) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_require_option(
                &workspace_root.join(crate::PNP_FILENAME),
                node_options,
            ),
        );
    }
    if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
        let package_map_path = config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_package_map_option(&package_map_path, node_options),
        );
    }
    extra_env
}

/// Whether the delegating CLI claims to have run the hook already.
///
/// Only the exact `true` the TypeScript CLI writes counts. Every other
/// value — unset, empty, `false` — runs the hook, so a stray assignment
/// in someone's environment cannot silently suppress it.
///
/// The marker reaches no process this install spawns — [`build_env`]
/// drops it — so a nested `pnpm install` started from a lifecycle script
/// still runs its own hook.
///
/// [`build_env`]: pnpm_executor::build_env
pub(super) fn dev_preinstall_already_ran() -> bool {
    std::env::var(DEV_PREINSTALL_ALREADY_RAN_ENV).is_ok_and(|value| value == "true")
}

/// Whether the delegating CLI claims to have run the root's `preinstall`
/// already, under the same rules as [`dev_preinstall_already_ran`].
pub(super) fn root_preinstall_already_ran() -> bool {
    std::env::var(ROOT_PREINSTALL_ALREADY_RAN_ENV).is_ok_and(|value| value == "true")
}

/// Run one of the root project's pre-resolution hooks — `run` is
/// [`pnpm_executor::run_dev_preinstall_hook`] or
/// [`pnpm_executor::run_root_preinstall_hook`].
///
/// `pnpm:devPreinstall` exists so a workspace can prepare state that
/// resolution or linking depends on — next.js creates the placeholder
/// `next` bin its other packages link against — and `preinstall` so a
/// guard can refuse the install before it changes anything, so both run
/// from the lockfile directory before either, and only for the root
/// project.
pub(super) fn run_root_hook(
    config: &Config,
    workspace_root: &Path,
    run: fn(&RunPostinstallHooks<'_>) -> Result<bool, LifecycleScriptError>,
) -> Result<bool, InstallError> {
    run_project_stages(
        config,
        workspace_root,
        workspace_root,
        config.extra_env_with_node_options(),
        run,
    )
    .map_err(InstallError::PreResolutionLifecycleScript)
}

/// Run `stages` of workspace projects' own lifecycle scripts as soon as
/// their dependency projects settle.
pub(super) fn run_projects_lifecycle_scripts<Reporter: self::Reporter>(
    project_graph: &ProjectLifecycleGraph<'_>,
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
    root_preinstall_ran: bool,
    stages: &[&str],
) -> Result<(), InstallError> {
    let runner = ProjectScriptRunner::new(config, node_linker, workspace_root, root_preinstall_ran);
    let first_error: Mutex<Option<InstallError>> = Mutex::new(None);
    let on_node_skipped: fn(&PathBuf) = |_| {};
    let run_node = |project_dir: PathBuf| {
        let project = &project_graph.projects_by_dir[&project_dir];
        if !project_requires_lifecycle_scripts(&project.0, project.1, stages) {
            return TaskCompletion::Passed;
        }
        match runner.run::<Reporter>(&project.0, project.1, stages) {
            Ok(()) => TaskCompletion::Passed,
            Err(error) => {
                first_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_or_insert(error);
                TaskCompletion::Failed
            }
        }
    };
    schedule_graph(
        &project_graph.dependencies,
        &ScheduleGraphOptions {
            concurrency: crate::script_thread_count(
                config.child_concurrency,
                project_graph.dependencies.len(),
            ),
            bail: true,
            continue_on_failure: false,
            run_node: &run_node,
            on_node_skipped: &on_node_skipped,
        },
    )
    .map_err(InstallError::ProjectLifecycleThreadPool)?;
    first_error
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .map_or(Ok(()), Err)
}
