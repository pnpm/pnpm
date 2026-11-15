//! A dependency structure matrix: the units of a graph in rows and columns
//! in the same order, a mark where the row depends on the column, and how
//! often each pair changed in the same commit.

use crate::{
    cochange::CoChange,
    graph::{CrateGraph, Edge},
    layers::Layers,
    modules::TopLevelGraph,
};
use std::collections::{BTreeMap, BTreeSet};

/// How a row depends on a column, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    /// Only through dev-dependencies.
    TestsOnly,
    /// Within the row's layer or downward.
    Linked,
    /// Upward, accepted by an `[[exception]]`.
    Excepted,
    /// Upward and not accepted.
    Upward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub mark: Mark,
    /// The dependency kinds, or the number of references between modules.
    pub detail: String,
}

/// Consecutive rows and columns, such as the crates of one layer.
#[derive(Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug)]
pub struct Matrix {
    pub title: String,
    /// The rows and columns in order, bottom layer first.
    pub groups: Vec<Group>,
    /// Keyed by `(row, column)`, where the row depends on the column.
    pub dependencies: BTreeMap<(String, String), Dependency>,
    pub co_change: CoChange,
}

impl Matrix {
    /// Every row in order.
    pub fn members(&self) -> impl Iterator<Item = &String> {
        self.groups.iter().flat_map(|group| &group.members)
    }

    /// The pairs of members that changed together in at least one commit
    /// and do not depend on each other in either direction, most commits
    /// first.
    pub fn independent_pairs(&self) -> Vec<(&str, &str, usize)> {
        let members = self.members().collect::<BTreeSet<_>>();
        let is_independent = |(left, right): &(String, String)| {
            members.contains(left)
                && members.contains(right)
                && !self.dependencies.contains_key(&(left.clone(), right.clone()))
                && !self.dependencies.contains_key(&(right.clone(), left.clone()))
        };
        let mut pairs = self.co_change.pairs
            .iter()
            .filter(|(pair, _)| is_independent(pair))
            .map(|((left, right), count)| (left.as_str(), right.as_str(), *count))
            .collect::<Vec<_>>();
        pairs.sort_by_key(|&(left, right, count)| (std::cmp::Reverse(count), left, right));
        pairs
    }
}

/// The workspace crates, grouped by layer from the bottom layer up and by
/// name within a layer. Crates in no layer come last.
pub fn crates(graph: &CrateGraph, layers: &Layers, co_change: CoChange) -> Matrix {
    let mut groups = layers
        .names()
        .iter()
        .map(|name| Group { name: name.clone(), members: Vec::new() })
        .collect::<Vec<_>>();
    let mut unassigned = Group { name: "no layer".to_owned(), members: Vec::new() };
    for name in &graph.crates {
        let group = match layers.rank_of(name) {
            Some(rank) => &mut groups[rank],
            None => &mut unassigned,
        };
        group.members.push(name.clone());
    }
    groups.push(unassigned);
    groups.retain(|group| !group.members.is_empty());
    Matrix {
        title: "Workspace crates".to_owned(),
        groups,
        dependencies: crate_dependencies(graph, layers),
        co_change,
    }
}

fn crate_dependencies(
    graph: &CrateGraph,
    layers: &Layers,
) -> BTreeMap<(String, String), Dependency> {
    let mut kinds = BTreeMap::<_, Vec<&str>>::new();
    let mut marks = BTreeMap::new();
    for edge in &graph.edges {
        let key = (edge.from.clone(), edge.to.clone());
        kinds
            .entry(key.clone())
            .or_default()
            .push(edge.kind.label());
        let mark = mark_of(edge, layers);
        let strongest = marks.entry(key).or_insert(mark);
        *strongest = mark.max(*strongest);
    }
    marks
        .into_iter()
        .map(|(key, mark)| {
            let detail = kinds[&key].join(", ");
            (key, Dependency { mark, detail })
        })
        .collect()
}

fn mark_of(edge: &Edge, layers: &Layers) -> Mark {
    if !edge.kind.is_linked() {
        return Mark::TestsOnly;
    }
    match layers.is_upward(edge) {
        Some(true) if layers.is_excepted(edge) => Mark::Excepted,
        Some(true) => Mark::Upward,
        _ => Mark::Linked,
    }
}

/// A crate's top-level modules by name. A module depends on another when
/// it names it.
pub fn modules(crate_name: &str, graph: &TopLevelGraph, co_change: CoChange) -> Matrix {
    let members = graph.code_lines
        .keys()
        .cloned()
        .collect();
    let dependencies = graph.edges
        .iter()
        .map(|(key, count)| {
            let noun = if *count == 1 { "reference" } else { "references" };
            let detail = format!("{count} {noun}");
            (key.clone(), Dependency { mark: Mark::Linked, detail })
        })
        .collect();
    Matrix {
        title: format!("Top-level modules of {crate_name}"),
        groups: vec![Group { name: crate_name.to_owned(), members }],
        dependencies,
        co_change,
    }
}
