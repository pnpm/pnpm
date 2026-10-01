//! The workspace-project-graph walks the recursive commands share: the
//! dependency edges among the `--filter`-selected projects, their topological
//! order, and the workspace dependencies a selection reaches.

use indexmap::IndexMap;
use pnpm_package_manager::{GraphSequencerResult, graph_sequencer};
use pnpm_workspace_projects_graph::ProjectGraph;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

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
            (project_dir.clone(), sorted_dependencies(selected, full_graph, project_dir, &sorted))
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
fn sequence_graph_by_project<'g, Pkg: 'g>(
    projects_graph: &ProjectGraph<Pkg>,
    full_graph_for: impl Fn(&Path) -> &'g ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    let sorted_dirs: Vec<PathBuf> = projects_graph.keys().cloned().collect();
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
fn sorted_dependencies<Pkg>(
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

/// `project_dirs` together with every workspace project they depend on,
/// directly or transitively.
///
/// The install a gated command spawns selects the dependencies of the projects
/// the command selected, so a project whose workspace dependency has no modules
/// directory is not up to date either. `all` is the graph the walk reads;
/// a project named by `prod_only_selected` reads `prod_all` instead, as
/// [`filtered_projects_dependencies`] does, so a dev-only workspace dependency
/// of a `--filter-prod` selection is not required.
///
/// Each graph keeps one visited set across all roots, so a dependency shared
/// by several selected projects is walked once.
pub fn projects_with_workspace_dependencies<'graph, 'dirs, Pkg>(
    project_dirs: impl IntoIterator<Item = &'dirs Path>,
    all: &'graph ProjectGraph<Pkg>,
    prod_all: Option<&'graph ProjectGraph<Pkg>>,
    prod_only_selected: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut all_walk = DependencyWalk::new(all);
    let mut prod_walk = prod_all.map(DependencyWalk::new);
    let mut outside_graph: Vec<&Path> = Vec::new();
    for project_dir in project_dirs {
        let walk = match &mut prod_walk {
            Some(prod_walk) if prod_only_selected.contains(project_dir) => prod_walk,
            _ => &mut all_walk,
        };
        if !walk.visit_from(project_dir) {
            outside_graph.push(project_dir);
        }
    }
    let mut collected: rustc_hash::FxHashSet<&Path> = rustc_hash::FxHashSet::default();
    all_walk.visited
        .into_iter()
        .chain(prod_walk.into_iter().flat_map(|walk| walk.visited))
        .chain(outside_graph)
        .filter(|project_dir| collected.insert(project_dir))
        .map(Path::to_path_buf)
        .collect()
}

/// A walk along the dependency edges of one graph, from any number of roots.
struct DependencyWalk<'graph, Pkg> {
    graph: &'graph ProjectGraph<Pkg>,
    seen: rustc_hash::FxHashSet<&'graph Path>,
    /// The projects reached, in the order they were first reached.
    visited: Vec<&'graph Path>,
}

impl<'graph, Pkg> DependencyWalk<'graph, Pkg> {
    fn new(graph: &'graph ProjectGraph<Pkg>) -> Self {
        DependencyWalk { graph, seen: rustc_hash::FxHashSet::default(), visited: Vec::new() }
    }

    /// Visit `root` and every project reachable from it that no earlier root
    /// reached. Returns `false`, visiting nothing, when `root` is not in the
    /// graph.
    fn visit_from(&mut self, root: &Path) -> bool {
        let Some((root, node)) = self.graph.get_key_value(root) else {
            return false;
        };
        if !self.seen.insert(root.as_path()) {
            return true;
        }
        self.visited.push(root.as_path());
        let mut stack: Vec<&'graph Path> = node.dependencies
            .iter()
            .map(PathBuf::as_path)
            .collect();
        while let Some(project_dir) = stack.pop() {
            if !self.seen.insert(project_dir) {
                continue;
            }
            self.visited.push(project_dir);
            if let Some(node) = self.graph.get(project_dir) {
                stack.extend(node.dependencies.iter().map(PathBuf::as_path));
            }
        }
        true
    }
}
