use super::{
    Config, DEV_PREINSTALL_ALREADY_RAN_ENV, DependencyGroup, ExecScriptsPrependNodePath, HashMap,
    HashSet, InstallError, Lockfile, NodeLinker, PackageManifest, Path, PathBuf, Reporter,
    RunPostinstallHooks, link_project_bins, project_requires_lifecycle_scripts,
    run_dev_preinstall_hook, run_project_lifecycle_scripts,
};
use rayon::prelude::*;

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

pub(super) fn order_project_lifecycle_groups<'a>(
    projects: &[(PathBuf, &'a PackageManifest)],
    ordered_groups: Option<&[Vec<PathBuf>]>,
    workspace_root: &Path,
    lockfile: Option<&Lockfile>,
) -> Result<Vec<Vec<(PathBuf, &'a PackageManifest)>>, InstallError> {
    let normalized_project_dirs = projects
        .iter()
        .map(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir))
        .collect::<Vec<_>>();
    let grouped_dirs = ordered_groups.map(|groups| {
        groups
            .iter()
            .flatten()
            .map(|project_dir| pnpm_fs::lexical_normalize(project_dir))
            .collect::<HashSet<_>>()
    });
    let explicit_groups_cover_projects = grouped_dirs.as_ref().is_some_and(|grouped_dirs| {
        normalized_project_dirs.iter().all(|project_dir| grouped_dirs.contains(project_dir))
    });
    let lockfile_groups;
    let fallback_groups;
    let ordered_groups = if explicit_groups_cover_projects {
        ordered_groups.expect("checked as present")
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
            .collect();
        lockfile_groups = crate::graph_sequencer(&graph, &included).chunks;
        &lockfile_groups
    } else if ordered_groups.is_some() {
        return Err(InstallError::ProjectLifecycleOrder {
            projects: normalized_project_dirs
                .iter()
                .filter(|project_dir| {
                    !grouped_dirs
                        .as_ref()
                        .expect("ordered groups are present")
                        .contains(*project_dir)
                })
                .map(|project_dir| project_dir.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        });
    } else {
        fallback_groups = normalized_project_dirs
            .iter()
            .cloned()
            .map(|project_dir| vec![project_dir])
            .collect::<Vec<_>>();
        &fallback_groups
    };
    let projects_by_dir = projects
        .iter()
        .map(|project| (pnpm_fs::lexical_normalize(&project.0), project))
        .collect::<HashMap<_, _>>();
    let mut included = HashSet::with_capacity(projects.len());
    let groups = ordered_groups
        .iter()
        .filter_map(|dirs| {
            let group = dirs
                .iter()
                .filter_map(|dir| {
                    projects_by_dir.get(&pnpm_fs::lexical_normalize(dir)).map(|project| {
                        included.insert(project.0.clone());
                        (*project).clone()
                    })
                })
                .collect::<Vec<_>>();
            (!group.is_empty()).then_some(group)
        })
        .collect::<Vec<_>>();
    let missing_projects = projects
        .iter()
        .filter(|(project_dir, _)| !included.contains(project_dir))
        .map(|(project_dir, _)| project_dir.display().to_string())
        .collect::<Vec<_>>();
    if !missing_projects.is_empty() {
        return Err(InstallError::ProjectLifecycleOrder { projects: missing_projects.join(", ") });
    }
    Ok(groups
        .into_iter()
        .filter_map(|group| {
            let group = group
                .into_iter()
                .filter(|(project_dir, manifest)| {
                    project_requires_lifecycle_scripts(project_dir, manifest)
                })
                .collect::<Vec<_>>();
            (!group.is_empty()).then_some(group)
        })
        .collect())
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

/// [`Config::extra_env_with_node_options`] plus the `NODE_OPTIONS` entry pointing Node at
/// `node_modules/.package-map.json` when the user opted into the
/// experimental package map. pnpm adds it only once it links and builds,
/// which is why `pnpm:devPreinstall` — running before the file exists —
/// takes the plain [`Config::extra_env_with_node_options`].
pub(super) fn project_lifecycle_extra_env(
    config: &Config,
    node_linker: NodeLinker,
) -> HashMap<String, String> {
    let mut extra_env = config.extra_env_with_node_options();
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
        optional: false,
    })
    .map(drop)
    .map_err(InstallError::ProjectLifecycleScript)
}

/// Run workspace projects' own lifecycle scripts in topological build
/// groups. Projects within one group run concurrently; each group settles
/// before the next starts. Every project re-links its bins immediately
/// before its scripts, after dependency projects' scripts from earlier
/// groups have had a chance to create new bin files.
pub(super) fn run_projects_lifecycle_scripts<Reporter: self::Reporter>(
    project_groups: &[Vec<(PathBuf, &PackageManifest)>],
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> Result<(), InstallError> {
    let modules_dir_basename = modules_dir_basename(config);
    let scripts_prepend_node_path = exec_scripts_prepend_node_path(config);
    let extra_env = project_lifecycle_extra_env(config, node_linker);
    let max_group_size = project_groups.iter().map(Vec::len).max().unwrap_or(0);
    let extra_node_paths = crate::shim_extra_node_paths(config, node_linker);
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
            link_project_bins(&root_modules_dir, &direct_dep_names, &extra_node_paths)
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
                optional: false,
            })
            .map_err(InstallError::ProjectLifecycleScript)?;
            Ok(())
        };
    if max_group_size <= 1 {
        for group in project_groups {
            for project in group {
                run_project(project)?;
            }
        }
        return Ok(());
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(crate::script_thread_count(config.child_concurrency, max_group_size))
        .build()
        .map_err(InstallError::ProjectLifecycleThreadPool)?;
    for group in project_groups {
        pool.install(|| group.par_iter().try_for_each(run_project))?;
    }
    Ok(())
}
