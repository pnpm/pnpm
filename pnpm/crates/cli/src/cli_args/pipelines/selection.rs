use super::{
    Arc, AutoExcludeRoot, Config, HashSet, IndexMap, InstallFamilySelection, Path, PathBuf,
    PathNode, active_manifest_is_standin, apply_runtime_on_fail, discover_workspace_projects,
    graph_sequencer, install_dirs, project_dependencies, select_recursive_projects,
};

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

pub(super) fn select_workspace_projects_with_cycles(
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

    let workspace_root = cfg.workspace_dir
        .as_deref()
        .unwrap_or(prefix)
        .to_path_buf();
    let (mut projects, workspace_patterns) = discover_workspace_projects(&workspace_root, cfg)?;
    apply_runtime_on_fail(cfg, &mut projects);
    let selection = select_recursive_projects(
        &projects,
        cfg,
        prefix,
        if auto_exclude_root {
            AutoExcludeRoot::Enabled {
                workspace_patterns: workspace_patterns.as_deref(),
            }
        } else {
            AutoExcludeRoot::Disabled
        },
    )?;
    let SelectionGraph {
        project_dependencies,
        ordered_dirs,
        selected_dirs,
        workspace_cycles,
    } = selection_graph(&selection, cfg, recursive_sort, precompute_workspace_cycles);

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
pub(super) fn sequence_project_dependencies(
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

pub(super) fn precomputed_workspace_cycles(
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    cfg: &Config,
    precompute_workspace_cycles: bool,
) -> Option<Vec<Vec<PathBuf>>> {
    (precompute_workspace_cycles && selection.all.is_none() && !cfg.ignore_workspace_cycles).then(
        || pnpm_package_manager::workspace_cycles(&selection.selected).unwrap_or_default(),
    )
}

struct SelectionGraph {
    project_dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
    ordered_dirs: Vec<PathBuf>,
    selected_dirs: Arc<HashSet<PathBuf>>,
    workspace_cycles: Option<Vec<Vec<PathBuf>>>,
}

fn selection_graph(
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    cfg: &Config,
    recursive_sort: bool,
    precompute_workspace_cycles: bool,
) -> SelectionGraph {
    let workspace_cycles =
        precomputed_workspace_cycles(selection, cfg, precompute_workspace_cycles);
    let project_dependencies = project_dependencies(selection, recursive_sort);
    let ordered_dirs = sequence_project_dependencies(&project_dependencies);
    let selected_dirs: Arc<HashSet<PathBuf>> = Arc::new(
        selection.selected
            .keys()
            .cloned()
            .collect(),
    );
    SelectionGraph {
        project_dependencies,
        ordered_dirs,
        selected_dirs,
        workspace_cycles,
    }
}
