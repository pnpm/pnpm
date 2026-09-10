use super::{HashMap, IndexSet, Path, PathBuf, ProjectGraph, ProjectSelector};

/// The selector modifiers that drive [`WalkState::select_entries`]. A
/// `[<since>]` selector selects its test-only-changed projects through
/// a copy with `include_dependents` suppressed.
#[derive(Clone, Copy)]
pub(super) struct WalkFlags {
    pub(super) include_dependencies: bool,
    pub(super) include_dependents: bool,
    pub(super) exclude_self: bool,
}

impl WalkFlags {
    pub(super) fn of(selector: &ProjectSelector) -> Self {
        WalkFlags {
            include_dependencies: selector.include_dependencies,
            include_dependents: selector.include_dependents,
            exclude_self: selector.exclude_self,
        }
    }
}

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
        flags: WalkFlags,
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
            let dependents: Vec<PathBuf> = self.walked_dependents.iter().cloned().collect();
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

/// Walk the subgraph reachable from `next_node_ids`.
///
/// `adj` returns the adjacency list (forward dependencies or reversed
/// dependents) for a node; recursion always re-includes the visited
/// children regardless of the top-level `include_root`.
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
            pick_subgraph(adj, &children, walked, true);
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
            reversed.entry(dependency.clone()).or_default().push(dependent.clone());
        }
    }
    reversed
}
