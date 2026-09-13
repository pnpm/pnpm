use super::{
    GraphPkg, GraphSequencerResult, HashMap, HashSet, IndexMap, Path, PathBuf, ProjectGraph,
    ResumeFromNotFound, graph_sequencer,
};
use pnpm_workspace_projects_graph::BaseProject as _;
use rayon::prelude::*;

/// The dependency edges among the `--filter`-selected projects, resolved
/// through the full workspace graph so a relationship between two selected
/// projects via an unselected one becomes a direct edge. Keys keep the
/// selection order.
pub fn filtered_projects_dependencies<Pkg: Sync>(
    selected: &ProjectGraph<Pkg>,
    all: &ProjectGraph<Pkg>,
    prod_all: Option<&ProjectGraph<Pkg>>,
    prod_only_selected: &HashSet<PathBuf>,
) -> IndexMap<PathBuf, Vec<PathBuf>> {
    let sorted: HashSet<&Path> = selected
        .keys()
        .map(PathBuf::as_path)
        .collect();
    // Each project's tunneling walk reads only shared references, so
    // the projects fan out across the rayon pool; collecting the
    // parallel iterator into a `Vec` keeps the selection order.
    selected
        .keys()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|&project_dir| {
            let full_graph = match prod_all {
                Some(prod_all) if prod_only_selected.contains(project_dir) => prod_all,
                _ => all,
            };
            (
                project_dir.clone(),
                sorted_dependencies(selected, full_graph, project_dir, &sorted),
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

/// Sequence `projects_graph` into one deterministic topological order,
/// resolving transitive edges through `full_projects_graph`.
pub fn sequence_graph<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    full_projects_graph: &ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    sequence_graph_by_project(projects_graph, |_| full_projects_graph)
}

/// Sequence `projects_graph`, resolving each project's transitive edges
/// through the full graph that `full_graph_for` returns for it. A
/// `--filter-prod` selection routes its projects to the prod-pruned graph so
/// pruned dev edges stay pruned, while regular projects route to the full
/// graph.
pub(super) fn sequence_graph_by_project<'g, Pkg: 'g>(
    projects_graph: &ProjectGraph<Pkg>,
    full_graph_for: impl Fn(&Path) -> &'g ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    let sorted_dirs: Vec<PathBuf> = projects_graph
        .keys()
        .cloned()
        .collect();
    let sorted: HashSet<&Path> = sorted_dirs
        .iter()
        .map(PathBuf::as_path)
        .collect();
    let dependency_graph: HashMap<PathBuf, Vec<PathBuf>> = projects_graph
        .keys()
        .map(|project_dir| {
            let dependencies = sorted_dependencies(
                projects_graph,
                full_graph_for(project_dir),
                project_dir,
                &sorted,
            );
            (project_dir.clone(), dependencies)
        })
        .collect();
    graph_sequencer(&dependency_graph, &sorted_dirs)
}

/// The dependencies of `project_dir` that are themselves in `sorted`, reached
/// by tunneling past any project outside `sorted`. A transitive dependency
/// between two sorted projects thus becomes a direct edge.
///
/// `project_dir`'s own edges are read from `projects_graph`, so a selection
/// that deliberately narrows them (e.g. a prod-only filter that drops dev
/// edges) is respected; `full_projects_graph` is consulted only to walk
/// through the projects outside `sorted`.
pub(super) fn sorted_dependencies<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    full_projects_graph: &ProjectGraph<Pkg>,
    project_dir: &Path,
    sorted: &HashSet<&Path>,
) -> Vec<PathBuf> {
    let mut dependencies: Vec<PathBuf> = Vec::new();
    // Borrowed paths and an FxHash set: this walk runs once per
    // selected project, and cloning every visited `PathBuf` into a
    // SipHash set dominated it on a workspace-scale graph.
    let mut visited: rustc_hash::FxHashSet<&Path> = rustc_hash::FxHashSet::default();
    let mut stack: Vec<&Path> = projects_graph
        .get(project_dir)
        .map(|node| {
            node.dependencies
                .iter()
                .map(PathBuf::as_path)
                .collect()
        })
        .unwrap_or_default();
    while let Some(dependency_dir) = stack.pop() {
        if dependency_dir == project_dir || !visited.insert(dependency_dir) {
            continue;
        }
        if sorted.contains(dependency_dir) {
            dependencies.push(dependency_dir.to_path_buf());
        } else if let Some(node) = full_projects_graph.get(dependency_dir) {
            stack.extend(node.dependencies.iter().map(PathBuf::as_path));
        }
    }
    dependencies
}

/// The project directory `--resume-from` names, located by manifest name;
/// an unknown name is a [`ResumeFromNotFound`] error. The invocation's
/// task for that project anchors the resumed task graph.
pub fn find_resume_root(
    resume_from: &str,
    graph: &ProjectGraph<GraphPkg<'_>>,
) -> Result<PathBuf, ResumeFromNotFound> {
    graph
        .iter()
        .find(|(_, node)| node.package.manifest_name() == Some(resume_from))
        .map(|(root, _)| root.clone())
        .ok_or_else(|| ResumeFromNotFound {
            resume_from: resume_from.to_string(),
        })
}
