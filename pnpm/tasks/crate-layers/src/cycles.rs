use crate::modules::TopLevelGraph;
use pnpm_strongly_connected_components::StronglyConnectedComponents;
use std::collections::{
    BTreeMap,
    BTreeSet,
};

/// The largest group of a crate's top-level modules that all reach each
/// other through references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleReport {
    pub crate_name: String,
    pub module_count: usize,
    pub total_lines: usize,
    /// The modules of the largest cycle, largest first. Empty without a cycle.
    pub largest: Vec<String>,
    pub largest_lines: usize,
}

impl CycleReport {
    pub fn new(crate_name: String, graph: &TopLevelGraph) -> Self {
        let biggest = components(graph)
            .into_iter()
            .max_by_key(Vec::len)
            .unwrap_or_default();
        let lines = |module: &String| {
            graph.code_lines
                .get(module)
                .copied()
                .unwrap_or_default()
        };
        let mut largest = if biggest.len() > 1 { biggest } else { Vec::new() };
        largest.sort_by_key(|module| (std::cmp::Reverse(lines(module)), module.clone()));
        Self {
            crate_name,
            module_count: graph.code_lines.len(),
            total_lines: graph.code_lines.values().sum(),
            largest_lines: largest.iter().map(lines).sum(),
            largest,
        }
    }

    /// The number of modules in the largest cycle, `0` when there is none.
    pub fn size(&self) -> usize {
        self.largest.len()
    }
}

/// Each module's successors, with an entry for every module that has code
/// lines or an edge of its own.
fn successors(graph: &TopLevelGraph) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut successors: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for module in graph.code_lines.keys() {
        successors.entry(module).or_default();
    }
    for (from, to) in graph.edges.keys() {
        successors
            .entry(from)
            .or_default()
            .insert(to);
    }
    successors
}

/// The strongly connected components of the module graph, in the order
/// `pnpm-strongly-connected-components` closes them.
fn components(graph: &TopLevelGraph) -> Vec<Vec<String>> {
    let successors = successors(graph);
    // A module that only an edge reaches comes after every module with an
    // entry, so the walk meets it through that edge, not as a root.
    let mut modules: Vec<&str> = successors.keys().copied().collect();
    let mut index_of: BTreeMap<&str, usize> = modules
        .iter()
        .enumerate()
        .map(|(index, module)| (*module, index))
        .collect();
    for target in successors.values().flatten() {
        if !index_of.contains_key(target) {
            index_of.insert(target, modules.len());
            modules.push(target);
        }
    }
    let adjacency: Vec<Vec<usize>> = modules
        .iter()
        .map(|module| {
            successors
                .get(module)
                .into_iter()
                .flatten()
                .map(|target| index_of[target])
                .collect()
        })
        .collect();
    let found = StronglyConnectedComponents::compute(&adjacency, &vec![false; modules.len()]);
    found
        .components()
        .iter()
        .map(|members| {
            members
                .iter()
                .map(|&member| modules[member].to_owned())
                .collect()
        })
        .collect()
}

/// Each crate's largest cycle size, for [`crate::ratchet::check`].
pub fn sizes<'report>(
    reports: impl IntoIterator<Item = &'report CycleReport>,
) -> BTreeMap<String, usize> {
    reports
        .into_iter()
        .map(|report| (report.crate_name.clone(), report.size()))
        .collect()
}
