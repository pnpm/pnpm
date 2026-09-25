pub(crate) use configuration::{
    apply_install_cli_config, derive_config_root, warn_about_config_root,
};
pub(crate) use install::InstallPipeline;
pub(crate) use maintenance::{DedupePipeline, PrunePipeline};
pub(crate) use mutation::{AddPipeline, DeployPipeline, RemovePipeline, UpdatePipeline};
pub(crate) use selection::{WorkspaceScope, select_workspace_projects};

use selection::{InstallFamily, select_install_family, select_install_family_plan};

use super::{
    add::AddArgs,
    dedupe::{self, DedupeArgs},
    deploy::DeployArgs,
    install::{InstallArgs, resolve_bool_override},
    prune::PruneArgs,
    recursive::{discover_workspace_projects, filtered_projects_dependencies},
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
    },
    config_deps, ecosystem_add, ecosystem_install,
    package_specifier::EcosystemPackageSpecifier,
    state::check_root_project_engine,
};
use indexmap::IndexMap;

use install::init_shared_state;

use miette::Context;

use pnpm_config::{Config, Host};
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{PathNode, graph_sequencer};
use pnpm_reporter::Reporter;
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

#[derive(Clone, Copy)]
enum RuntimePolicy {
    Always,
    Config(bool),
}

impl RuntimePolicy {
    fn use_manifest(self, config: &Config) -> bool {
        match self {
            Self::Always => true,
            Self::Config(no_runtime) => !(config.skip_runtimes || no_runtime),
        }
    }
}

async fn prepare_root_config<Reporter>(
    (manifest_path, config, config_root): (&Path, &mut Config, &Path),
    (frozen_lockfile, runtime_policy): (bool, RuntimePolicy),
) -> miette::Result<()>
where
    Reporter: self::Reporter,
{
    if !config_deps::may_update_config(config, config_root) {
        check_root_project_engine(manifest_path, config, runtime_policy.use_manifest(config))?;
    }
    config_deps::prepare::<Reporter>(config, config_root, frozen_lockfile).await?;
    check_root_project_engine(manifest_path, config, runtime_policy.use_manifest(config))?;
    Ok(())
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
    /// Whether the selection is every workspace project, so that the run
    /// leaves no project's lockfile behind its manifest.
    covers_workspace: bool,
}

impl DedicatedProjects {
    fn new(config: &Config, selection: InstallFamilySelection) -> Self {
        let names = project_names(config, &selection.projects);
        let normalized_root = pnpm_fs::lexical_normalize(&selection.workspace_root);
        let root_is_project = pnpm_package_manifest::project_manifest_path(
            &normalized_root,
            config.preferred_manifest_format,
        )
        .is_file();
        let covers_workspace = selection.projects
            .iter()
            .all(|project| selection.selected_dirs.contains(&project.root_dir))
            && (!root_is_project
                || selection.selected_dirs
                    .iter()
                    .any(|dir| pnpm_fs::lexical_normalize(dir) == normalized_root));
        DedicatedProjects { dependencies: selection.project_dependencies, names, covers_workspace }
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
            let name = project.manifest
                .value()
                .get("name")?
                .as_str()?;
            Some((project.root_dir.clone(), name.to_string()))
        })
        .collect()
}

/// The name `packageConfigs` addresses the project at `project_dir` by,
/// for a caller with no parsed manifest in hand. `None` when the setting
/// is unset: the name would have nothing to look up.
fn dedicated_project_name(config: &Config, project_dir: &Path) -> Option<String> {
    config.package_configs.as_ref()?;
    pnpm_workspace::read_project_name(project_dir, config.preferred_manifest_format)
}

struct DedicatedProjectRuns<'a> {
    config: &'a Config,
    projects: DedicatedProjects,
    require_lockfile: bool,
    http_client: Option<Arc<ThrottledClient>>,
    /// Whether the command may write the workspace manifest, so that the
    /// exclude-list prune each project's install skipped runs once all of
    /// them succeeded and they cover the workspace. See
    /// [`prune_after_dedicated_installs`].
    prune_excludes: bool,
}

impl DedicatedProjectRuns<'_> {
    async fn run<Runner, RunFuture>(self, run: Runner) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send,
    {
        self.run_projects(run).await?;
        if self.prune_excludes && self.projects.covers_workspace {
            prune_after_dedicated_installs(self.config)?;
        }
        Ok(())
    }

    async fn run_projects<Runner, RunFuture>(&self, run: Runner) -> miette::Result<()>
    where
        Runner: Fn(State) -> RunFuture + Sync,
        RunFuture: Future<Output = miette::Result<()>> + Send,
    {
        let first_error: std::sync::Mutex<Option<miette::Report>> = std::sync::Mutex::new(None);
        let config = self.config;
        let require_lockfile = self.require_lockfile;
        let http_client = &self.http_client;
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

/// The `minimumReleaseAgeExcludePrune` / `trustPolicyExcludePrune` pass
/// of a `sharedWorkspaceLockfile: false` workspace. Each project's install
/// skips it because its own lockfile cannot prove what a sibling resolves,
/// so it runs once here, after a run that installed every project. A
/// filtered run skips it: an unselected project's lockfile may lag behind
/// its manifest.
fn prune_after_dedicated_installs(config: &Config) -> miette::Result<()> {
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(());
    };
    pnpm_package_manager::prune_against_project_lockfiles(config, workspace_dir)
        .wrap_err("prune the workspace manifest")
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
                (
                    PathNode(key.as_path()),
                    value
                        .iter()
                        .map(|dir| PathNode(dir))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
        &project_dependencies
            .keys()
            .map(|dir| PathNode(dir))
            .collect::<Vec<_>>(),
    )
    .order
    .into_iter()
    .map(|node| node.0.to_path_buf())
    .collect()
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
    let mut dirs = selection.selected
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.into_iter()
        .map(|dir| (dir, Vec::new()))
        .collect()
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
    let manifest_path =
        pnpm_workspace::project_manifest_path(project_dir, cfg.preferred_manifest_format);
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

pub(in crate::cli_args) fn anchor_active_project(cfg: &mut Config, manifest_path: &Path) {
    let manifest_dir = manifest_path
        .parent()
        .expect("manifest path always has a parent dir")
        .to_path_buf();
    let name = dedicated_project_name(cfg, &manifest_dir);
    cfg.anchor_dedicated_project(&manifest_dir, name.as_deref());
}

/// The config through which a command finds the installed packages of the
/// active project. In a workspace whose projects keep their own lockfiles,
/// those are in the active project's modules directory, not the workspace
/// root's.
pub(in crate::cli_args) fn installed_project_config(
    config: &'static Config,
    manifest_path: &Path,
) -> &'static Config {
    if !keeps_project_lockfiles(config) {
        return config;
    }
    let mut config = config.clone();
    anchor_active_project(&mut config, manifest_path);
    Config::leak(config)
}

pub(in crate::cli_args) fn keeps_project_lockfiles(config: &Config) -> bool {
    !config.shares_one_lockfile() && config.workspace_dir.is_some()
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
    (precompute_workspace_cycles && selection.all.is_none() && !cfg.ignore_workspace_cycles).then(
        || pnpm_package_manager::workspace_cycles(&selection.selected).unwrap_or_default(),
    )
}

mod install;
mod nested_workspace_manifests;
mod selection;

mod mutation;

mod maintenance;

mod configuration;
