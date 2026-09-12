pub(crate) use configuration::{apply_install_cli_config, derive_config_root};
pub(crate) use install::InstallPipeline;
pub(crate) use maintenance::{DedupePipeline, PrunePipeline};
pub(crate) use mutation::{AddPipeline, DeployPipeline, RemovePipeline, UpdatePipeline};

use super::{
    add::AddArgs,
    dedupe::{self, DedupeArgs},
    deploy::DeployArgs,
    install::{InstallArgs, resolve_bool_override},
    package_manager::read_manifest_json,
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
        config_warnings::{warn_unapplied_package_configs, warn_unmatched_registry_options},
        legacy_pnpm_field::warn_ignored_pnpm_manifest_fields,
        override_version_references::warn_deprecated_override_version_references,
        reporter::{ReporterType, reporter_emit},
        yarn_workspaces_field::warn_unsupported_workspaces_field,
    },
    config_deps, ecosystem_add, ecosystem_install,
};
use configuration::apply_runtime_on_fail;

use indexmap::IndexMap;

use install::init_shared_state;

use miette::Context;

use pnpm_config::{Config, Host};
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{PathNode, graph_sequencer};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, ScopeLog};
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, schedule_graph_async,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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

impl InstallFamilySelection {
    pub(crate) fn selected_projects(&mut self) -> pnpm_package_manager::SelectedProjects<'_> {
        pnpm_package_manager::SelectedProjects {
            projects: &mut self.projects,
            project_dependencies: &self.project_dependencies,
            ordered_dirs: &self.ordered_dirs,
            selected_dirs: self.selected_dirs.as_ref(),
            install_dirs: self.install_dirs.as_ref(),
            active_manifest_is_standin: self.active_manifest_is_standin,
        }
    }
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
    PerProject(DedicatedProjects),
}

/// The projects of a `sharedWorkspaceLockfile: false` workspace that a
/// recursive / filtered command installs one by one.
pub(crate) struct DedicatedProjects {
    /// Which project must finish before which, keyed by project dir.
    dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
    /// The name each project is addressed by in `packageConfigs`, taken
    /// from the manifests the selection already parsed. Empty when the
    /// setting is unset, which is the only thing the names feed.
    names: HashMap<PathBuf, String>,
}

impl DedicatedProjects {
    fn new(config: &Config, selection: InstallFamilySelection) -> Self {
        let names = project_names(config, &selection.projects);
        DedicatedProjects { dependencies: selection.project_dependencies, names }
    }

    fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }
}

/// The declared name of every project that declares one, keyed by its
/// directory. Empty when `packageConfigs` is unset: addressing a project
/// by name is the only thing the map feeds.
pub(crate) fn project_names(
    config: &Config,
    projects: &[pnpm_workspace::Project],
) -> HashMap<PathBuf, String> {
    if config.package_configs.is_none() {
        return HashMap::new();
    }
    projects
        .iter()
        .filter_map(|project| {
            let name = project.manifest.value().get("name")?.as_str()?;
            Some((project.root_dir.clone(), name.to_string()))
        })
        .collect()
}

/// The name `packageConfigs` addresses the project at `project_dir` by,
/// for a caller with no parsed manifest in hand. `None` when the setting
/// is unset: the name would have nothing to look up.
fn dedicated_project_name(config: &Config, project_dir: &Path) -> Option<String> {
    config.package_configs.as_ref()?;
    pnpm_workspace::read_project_name(project_dir)
}

struct DedicatedProjectRuns<'a> {
    config: &'a Config,
    projects: DedicatedProjects,
    require_lockfile: bool,
    http_client: Option<Arc<ThrottledClient>>,
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
        let http_client = self.http_client;
        let names = &self.projects.names;
        let run = &run;
        let run_node = |project_dir: PathBuf| {
            let first_error = &first_error;
            let http_client = http_client.as_ref().map(Arc::clone);
            async move {
                let result = match init_dedicated_project_state(
                    config,
                    &project_dir,
                    names.get(&project_dir).map(String::as_str),
                    require_lockfile,
                    http_client,
                ) {
                    Ok(state) => run(state).await,
                    Err(error) => Err(error),
                };
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
        return Ok(InstallFamilyPlan::PerProject(DedicatedProjects::new(cfg, selection)));
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
    apply_runtime_on_fail(cfg, &mut projects);
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
        let workspace_cycles =
            precomputed_workspace_cycles(&selection, cfg, precompute_workspace_cycles);
        let project_dependencies = project_dependencies(&selection, recursive_sort);
        let ordered_dirs = sequence_project_dependencies(&project_dependencies);
        let selected_dirs: Arc<HashSet<PathBuf>> =
            Arc::new(selection.selected.keys().cloned().collect());
        (project_dependencies, ordered_dirs, selected_dirs, workspace_cycles)
    };

    let active_dir = manifest_path.parent().expect("manifest path always has a parent dir");
    let active_manifest_is_standin = active_manifest_is_standin(active_dir, &projects)?;
    let install_dirs = install_dirs(&selected_dirs, &projects, &workspace_root);

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

/// The selection in build order. Sequenced over borrowed paths: cloning a
/// workspace-scale edge map just to sort it cost more than the sort.
fn sequence_project_dependencies(
    project_dependencies: &IndexMap<PathBuf, Vec<PathBuf>>,
) -> Vec<PathBuf> {
    graph_sequencer(
        &project_dependencies
            .iter()
            .map(|(key, value)| {
                (PathNode(key.as_path()), value.iter().map(|dir| PathNode(dir)).collect::<Vec<_>>())
            })
            .collect(),
        &project_dependencies.keys().map(|dir| PathNode(dir)).collect::<Vec<_>>(),
    )
    .order
    .into_iter()
    .map(|node| node.0.to_path_buf())
    .collect()
}

/// Whether the active directory has no manifest of its own and is none of
/// the workspace's projects, so the manifest at hand stands in for one.
fn active_manifest_is_standin(
    active_dir: &Path,
    projects: &[pnpm_workspace::Project],
) -> miette::Result<bool> {
    let normalized_active_dir = pnpm_fs::lexical_normalize(active_dir);
    Ok(!active_dir.join("package.json").is_file()
        && pnpm_workspace::try_read_project_manifest(active_dir)
            .map_err(miette::Report::new)?
            .is_none()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_active_dir))
}

/// The selected projects plus the workspace root project, when the
/// workspace root is a project.
fn install_dirs(
    selected_dirs: &HashSet<PathBuf>,
    projects: &[pnpm_workspace::Project],
    workspace_root: &Path,
) -> HashSet<PathBuf> {
    let normalized_workspace_root = pnpm_fs::lexical_normalize(workspace_root);
    let mut install_dirs = selected_dirs.clone();
    if let Some(workspace_root_project) = projects
        .iter()
        .find(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_workspace_root)
    {
        install_dirs.insert(workspace_root_project.root_dir.clone());
    }
    install_dirs
}

/// The edges the sequencer orders the selection by. Without `--sort`
/// there are none, and the projects run in directory order.
fn project_dependencies(
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    recursive_sort: bool,
) -> IndexMap<PathBuf, Vec<PathBuf>> {
    if recursive_sort {
        return filtered_projects_dependencies(
            &selection.selected,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        );
    }
    let mut dirs = selection.selected.keys().cloned().collect::<Vec<_>>();
    dirs.sort();
    dirs.into_iter().map(|dir| (dir, Vec::new())).collect()
}

/// Build the project-anchored `State` for one project of a
/// `sharedWorkspaceLockfile: false` workspace: clone `cfg`, re-anchor its
/// output paths and per-project settings under `project_dir` via
/// [`Config::anchor_dedicated_project`], and initialize the state. The
/// clone is leaked because [`State::init`] needs a `&'static Config`; see
/// [`run_dedicated_lockfile_workspace_install`](install::run_dedicated_lockfile_workspace_install) for why the bounded leak
/// is acceptable.
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

fn anchor_active_project(cfg: &mut Config, manifest_path: &Path) {
    let manifest_dir =
        manifest_path.parent().expect("manifest path always has a parent dir").to_path_buf();
    let name = dedicated_project_name(cfg, &manifest_dir);
    cfg.anchor_dedicated_project(&manifest_dir, name.as_deref());
}

fn record_dedicated_result(
    first_error: &std::sync::Mutex<Option<miette::Report>>,
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

fn precomputed_workspace_cycles(
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    cfg: &Config,
    precompute_workspace_cycles: bool,
) -> Option<Vec<Vec<PathBuf>>> {
    (precompute_workspace_cycles && selection.all.is_none() && !cfg.ignore_workspace_cycles)
        .then(|| pnpm_package_manager::workspace_cycles(&selection.selected).unwrap_or_default())
}

mod install;

mod mutation;

mod maintenance;

mod configuration;
