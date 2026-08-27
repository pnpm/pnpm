use super::{
    Config, DEV_PREINSTALL_ALREADY_RAN_ENV, DependencyGroup, ExecScriptsPrependNodePath, HashMap,
    HashSet, InstallError, Lockfile, NodeLinker, PackageManifest, Path, PathBuf, Reporter,
    RunPostinstallHooks, link_project_bins, project_requires_lifecycle_scripts,
    run_dev_preinstall_hook, run_project_lifecycle_scripts,
};
use indexmap::IndexMap;
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
) -> Result<Option<Vec<pnpm_workspace::Project>>, pnpm_workspace::FindWorkspaceProjectsError> {
    let Some(manifest) = workspace_manifest else { return Ok(None) };
    let opts = pnpm_workspace::FindWorkspaceProjectsOpts {
        patterns: Some(pnpm_workspace::workspace_package_patterns(manifest)),
    };
    pnpm_workspace::find_workspace_projects(workspace_root, &opts).map(Some)
}

pub(super) struct ProjectLifecycleGraph<'a> {
    projects_by_dir: HashMap<PathBuf, (PathBuf, &'a PackageManifest)>,
    pub(super) dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
}

pub(super) fn project_lifecycle_graph<'a>(
    projects: &[(PathBuf, &'a PackageManifest)],
    ordered_dependencies: Option<&IndexMap<PathBuf, Vec<PathBuf>>>,
    workspace_root: &Path,
    lockfile: Option<&Lockfile>,
) -> Result<ProjectLifecycleGraph<'a>, InstallError> {
    let normalized_project_dirs = projects
        .iter()
        .map(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir))
        .collect::<Vec<_>>();
    let ordered_dirs = ordered_dependencies.map(|dependencies| {
        dependencies
            .keys()
            .map(|project_dir| pnpm_fs::lexical_normalize(project_dir))
            .collect::<HashSet<_>>()
    });
    let explicit_order_covers_projects = ordered_dirs.as_ref().is_some_and(|ordered_dirs| {
        normalized_project_dirs.iter().all(|project_dir| ordered_dirs.contains(project_dir))
    });
    let lockfile_dependencies;
    let fallback_dependencies;
    let dependencies = if explicit_order_covers_projects {
        ordered_dependencies.expect("checked as present")
    } else if let Some(lockfile) = lockfile {
        let included = normalized_project_dirs.clone();
        let included_set = included.iter().cloned().collect::<HashSet<_>>();
        let graph = projects
            .iter()
            .zip(&normalized_project_dirs)
            .map(|((project_dir, _), normalized_project_dir)| {
                let importer_id =
                    pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir);
                let dependencies = lockfile
                    .importers
                    .get(&importer_id)
                    .into_iter()
                    .flat_map(|snapshot| {
                        [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
                            .into_iter()
                            .filter_map(|group| snapshot.get_map_by_group(group))
                            .flat_map(|dependencies| dependencies.values())
                    })
                    .filter_map(|dependency| match &dependency.version {
                        pnpm_lockfile::ImporterDepVersion::Link(target) => {
                            Some(pnpm_fs::lexical_normalize(&project_dir.join(target)))
                        }
                        _ => None,
                    })
                    .filter(|target| included_set.contains(target))
                    .collect();
                (normalized_project_dir.clone(), dependencies)
            })
            .collect::<IndexMap<_, _>>();
        lockfile_dependencies = graph;
        &lockfile_dependencies
    } else if ordered_dependencies.is_some() {
        return Err(InstallError::ProjectLifecycleOrder {
            projects: normalized_project_dirs
                .iter()
                .filter(|project_dir| {
                    !ordered_dirs
                        .as_ref()
                        .expect("ordered dependencies are present")
                        .contains(*project_dir)
                })
                .map(|project_dir| project_dir.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        });
    } else {
        fallback_dependencies = normalized_project_dirs
            .iter()
            .cloned()
            .map(|project_dir| (project_dir, Vec::new()))
            .collect::<IndexMap<_, _>>();
        &fallback_dependencies
    };
    let projects_by_dir = projects
        .iter()
        .map(|project| (pnpm_fs::lexical_normalize(&project.0), project))
        .collect::<HashMap<_, _>>();
    let included: HashSet<PathBuf> = dependencies
        .keys()
        .map(|dir| pnpm_fs::lexical_normalize(dir))
        .filter(|dir| projects_by_dir.contains_key(dir))
        .collect();
    let missing_projects = projects
        .iter()
        .filter(|(project_dir, _)| !included.contains(&pnpm_fs::lexical_normalize(project_dir)))
        .map(|(project_dir, _)| project_dir.display().to_string())
        .collect::<Vec<_>>();
    if !missing_projects.is_empty() {
        return Err(InstallError::ProjectLifecycleOrder { projects: missing_projects.join(", ") });
    }
    let dependencies = dependencies
        .iter()
        .filter_map(|(dir, project_dependencies)| {
            let dir = pnpm_fs::lexical_normalize(dir);
            projects_by_dir.contains_key(&dir).then(|| {
                (
                    dir,
                    project_dependencies
                        .iter()
                        .map(|dependency| pnpm_fs::lexical_normalize(dependency))
                        .filter(|dependency| projects_by_dir.contains_key(dependency))
                        .collect(),
                )
            })
        })
        .collect();
    Ok(ProjectLifecycleGraph {
        projects_by_dir: projects_by_dir
            .into_iter()
            .map(|(dir, project)| (dir, project.clone()))
            .collect(),
        dependencies,
    })
}

pub(super) fn modules_dir_basename(config: &Config) -> &std::ffi::OsStr {
    config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"))
}

/// Same tri-state mapping the dependency-build path applies; see the doc
/// on [`pnpm_config::ScriptsPrependNodePath`].
pub(super) fn exec_scripts_prepend_node_path(config: &Config) -> ExecScriptsPrependNodePath {
    match config.scripts_prepend_node_path {
        pnpm_config::ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
        pnpm_config::ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
        pnpm_config::ScriptsPrependNodePath::WarnOnly => ExecScriptsPrependNodePath::WarnOnly,
    }
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

/// Run the root project's `pnpm:devPreinstall` script, if it defines one.
///
/// The hook exists so a workspace can prepare state that resolution or
/// linking depends on — next.js creates the placeholder `next` bin its
/// other packages link against — so it runs from the lockfile directory
/// before either, and only for the root project.
pub(super) fn run_dev_preinstall<Reporter: self::Reporter>(
    config: &Config,
    workspace_root: &Path,
) -> Result<(), InstallError> {
    let root_modules_dir = workspace_root.join(modules_dir_basename(config));
    let extra_env = config.extra_env_with_node_options();
    let dep_path = workspace_root.to_string_lossy();
    run_dev_preinstall_hook::<Reporter>(&RunPostinstallHooks {
        dep_path: &dep_path,
        pkg_root: workspace_root,
        root_modules_dir: &root_modules_dir,
        init_cwd: workspace_root,
        extra_bin_paths: &config.extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: Some(&config.user_agent),
        unsafe_perm: config.unsafe_perm,
        node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
        scripts_prepend_node_path: exec_scripts_prepend_node_path(config),
        script_shell: config.script_shell.as_deref().map(Path::new),
        shell_emulator: config.shell_emulator,
        optional: false,
    })
    .map(drop)
    .map_err(InstallError::ProjectLifecycleScript)
}

/// Run workspace projects' own lifecycle scripts as soon as their dependency
/// projects settle.
pub(super) fn run_projects_lifecycle_scripts<Reporter: self::Reporter>(
    project_graph: &ProjectLifecycleGraph<'_>,
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> Result<(), InstallError> {
    let modules_dir_basename = modules_dir_basename(config);
    let scripts_prepend_node_path = exec_scripts_prepend_node_path(config);
    let extra_env = project_lifecycle_extra_env(config, node_linker, workspace_root);
    let link_options = crate::shim_link_options(config, node_linker);
    let run_project =
        |(project_dir, manifest): &(PathBuf, &PackageManifest)| -> Result<(), InstallError> {
            let root_modules_dir = project_dir.join(modules_dir_basename);
            let mut direct_dep_names = Vec::new();
            let mut seen = HashSet::new();
            for (name, _) in manifest.dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]) {
                if seen.insert(name) {
                    direct_dep_names.push(name.to_string());
                }
            }
            link_project_bins(&root_modules_dir, &direct_dep_names, &link_options)
                .map_err(InstallError::ProjectBinLink)?;
            let dep_path = project_dir.to_string_lossy();
            run_project_lifecycle_scripts::<Reporter>(&RunPostinstallHooks {
                dep_path: &dep_path,
                pkg_root: project_dir,
                root_modules_dir: &root_modules_dir,
                init_cwd: workspace_root,
                extra_bin_paths: &config.extra_bin_paths,
                extra_env: &extra_env,
                node_execpath: None,
                npm_execpath: None,
                node_gyp_path: None,
                user_agent: Some(&config.user_agent),
                unsafe_perm: config.unsafe_perm,
                node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
                scripts_prepend_node_path,
                script_shell: config.script_shell.as_deref().map(Path::new),
                shell_emulator: config.shell_emulator,
                optional: false,
            })
            .map_err(InstallError::ProjectLifecycleScript)?;
            Ok(())
        };
    let first_error: Mutex<Option<InstallError>> = Mutex::new(None);
    let on_node_skipped: fn(&PathBuf) = |_| {};
    let run_node = |project_dir: PathBuf| {
        let project = &project_graph.projects_by_dir[&project_dir];
        if !project_requires_lifecycle_scripts(&project.0, project.1) {
            return TaskCompletion::Passed;
        }
        match run_project(project) {
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
    first_error.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner).map_or(Ok(()), Err)
}
