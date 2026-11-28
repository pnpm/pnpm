use super::{
    HashMap,
    Path,
    PathBuf,
    ProjectGraph,
};
use crate::parse_project_selector::DependencyTraversal;
use indexmap::IndexSet;

/// Accumulates the projects the selectors of one [`filter_graph`](crate::filter::filter_graph) run
/// pick, in the buckets whose union (dependencies, dependents,
/// dependents' dependencies, then cherry-picks) fixes the selection
/// order.
#[derive(Default)]
pub(super) struct WalkState {
    pub(super) cherry_picked: Vec<PathBuf>,
    pub(super) walked_dependencies: IndexSet<PathBuf>,
    pub(super) walked_dependents: IndexSet<PathBuf>,
    pub(super) walked_dependents_dependencies: IndexSet<PathBuf>,
}

impl WalkState {
    pub(super) fn select_entries<Forward, Reverse>(
        &mut self,
        flags: DependencyTraversal,
        entry_projects: &[PathBuf],
        forward: &Forward,
        reverse: &Reverse,
    ) where
        Forward: Fn(&Path) -> Option<Vec<PathBuf>>,
        Reverse: Fn(&Path) -> Option<Vec<PathBuf>>,
    {
        let include_root = !flags.exclude_self;
        if flags.include_dependencies {
            pick_subgraph(forward, entry_projects, &mut self.walked_dependencies, include_root);
        }
        if flags.include_dependents {
            pick_subgraph(reverse, entry_projects, &mut self.walked_dependents, include_root);
        }
        if flags.include_dependencies && flags.include_dependents {
            let dependents: Vec<PathBuf> = self.walked_dependents
                .iter()
                .cloned()
                .collect();
            pick_subgraph(forward, &dependents, &mut self.walked_dependents_dependencies, false);
        }
        if !flags.include_dependencies && !flags.include_dependents {
            self.cherry_picked.extend(entry_projects.iter().cloned());
        }
    }

    pub(super) fn into_selected(self) -> Vec<PathBuf> {
        let mut walked: IndexSet<PathBuf> = IndexSet::new();
        walked.extend(self.walked_dependencies);
        walked.extend(self.walked_dependents);
        walked.extend(self.walked_dependents_dependencies);
        walked.extend(self.cherry_picked);
        walked.into_iter().collect()
    }
}

/// Walk the subgraph reachable from `next_node_ids` depth first, adding
/// to `walked` each node below them the first time the walk meets it, and
/// each of `next_node_ids` too when `include_root` is set.
///
/// `adj` returns the adjacency list (forward dependencies or reversed
/// dependents) for a node.
fn pick_subgraph<Adjacency>(
    adj: &Adjacency,
    next_node_ids: &[PathBuf],
    walked: &mut IndexSet<PathBuf>,
    include_root: bool,
) where
    Adjacency: Fn(&Path) -> Option<Vec<PathBuf>>,
{
    for next_node_id in next_node_ids {
        if walked.contains(next_node_id) {
            continue;
        }
        if include_root {
            walked.insert(next_node_id.clone());
        }
        if let Some(children) = adj(next_node_id) {
            pick_descendants(adj, children, walked);
        }
    }
}

/// Add each node `children` reach, themselves included, that `walked`
/// does not hold yet, depth first. The walk keeps its frames on a stack of
/// its own, so a chain of projects of any length is walked without
/// recursion.
fn pick_descendants<Adjacency>(
    adj: &Adjacency,
    children: Vec<PathBuf>,
    walked: &mut IndexSet<PathBuf>,
) where
    Adjacency: Fn(&Path) -> Option<Vec<PathBuf>>,
{
    // Each frame holds one node's children and the index of the next.
    let mut frames: Vec<(Vec<PathBuf>, usize)> = vec![(children, 0)];
    while let Some((children, next)) = frames.last_mut() {
        let Some(child) = children.get(*next).cloned() else {
            frames.pop();
            continue;
        };
        *next += 1;
        if walked.insert(child.clone()) {
            frames.extend(adj(&child).map(|grandchildren| (grandchildren, 0)));
        }
    }
}

/// Invert edges so a node maps to the projects that depend on it.
pub(super) fn reverse_graph<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut reversed: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (dependent, node) in projects_graph {
        for dependency in &node.dependencies {
            reversed
                .entry(dependency.clone())
                .or_default()
                .push(dependent.clone());
        }
    }
    reversed
}

#[cfg(test)]
mod tests;
