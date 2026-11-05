use crate::modules::TopLevelGraph;
use std::collections::{BTreeMap, BTreeSet};

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
    pub fn new(crate_name: &str, graph: &TopLevelGraph) -> Self {
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
            crate_name: crate_name.to_owned(),
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

/// The strongly connected components of the module graph (Tarjan).
fn components(graph: &TopLevelGraph) -> Vec<Vec<String>> {
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
    let mut tarjan = Tarjan {
        successors: &successors,
        next_index: 0,
        index: BTreeMap::new(),
        low_link: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        components: Vec::new(),
    };
    for module in successors.keys() {
        if !tarjan.index.contains_key(module) {
            tarjan.visit(module);
        }
    }
    tarjan.components
}

struct Tarjan<'graph> {
    successors: &'graph BTreeMap<&'graph str, BTreeSet<&'graph str>>,
    next_index: usize,
    index: BTreeMap<&'graph str, usize>,
    low_link: BTreeMap<&'graph str, usize>,
    stack: Vec<&'graph str>,
    on_stack: BTreeSet<&'graph str>,
    components: Vec<Vec<String>>,
}

impl<'graph> Tarjan<'graph> {
    fn visit(&mut self, module: &'graph str) {
        self.index.insert(module, self.next_index);
        self.low_link.insert(module, self.next_index);
        self.next_index += 1;
        self.stack.push(module);
        self.on_stack.insert(module);
        let graph = self.successors;
        let successors = graph
            .get(module)
            .into_iter()
            .flatten()
            .copied();
        for successor in successors {
            if !self.index.contains_key(successor) {
                self.visit(successor);
                self.lower(module, self.low_link[successor]);
            } else if self.on_stack.contains(successor) {
                self.lower(module, self.index[successor]);
            }
        }
        if self.low_link[module] == self.index[module] {
            self.pop_component(module);
        }
    }

    fn lower(&mut self, module: &'graph str, candidate: usize) {
        let low = self.low_link.entry(module).or_default();
        *low = (*low).min(candidate);
    }

    fn pop_component(&mut self, root: &str) {
        let mut component = Vec::new();
        while let Some(member) = self.stack.pop() {
            self.on_stack.remove(member);
            component.push(member.to_owned());
            if member == root {
                break;
            }
        }
        self.components.push(component);
    }
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
