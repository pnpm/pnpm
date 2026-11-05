use crate::{graph::CrateGraph, layers::Layers, problem::Problem};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

/// The workspace crates each crate links: its normal and build dependencies,
/// directly and through other workspace crates.
#[derive(Debug, PartialEq, Eq)]
pub struct Reach {
    pub direct: BTreeMap<String, BTreeSet<String>>,
    pub transitive: BTreeMap<String, BTreeSet<String>>,
}

impl Reach {
    pub fn new(graph: &CrateGraph) -> Self {
        let mut direct: BTreeMap<String, BTreeSet<String>> = graph.crates
            .iter()
            .map(|name| (name.clone(), BTreeSet::new()))
            .collect();
        for edge in graph.edges.iter().filter(|edge| edge.kind.is_linked()) {
            direct
                .entry(edge.from.clone())
                .or_default()
                .insert(edge.to.clone());
        }
        let transitive = direct
            .keys()
            .map(|name| (name.clone(), reachable(&direct, name)))
            .collect();
        Self { direct, transitive }
    }

    pub fn direct_counts(&self) -> BTreeMap<String, usize> {
        self.direct
            .iter()
            .map(|(name, set)| (name.clone(), set.len()))
            .collect()
    }

    /// The crates whose transitive dependencies exceed their layer's
    /// `max_transitive_dependencies`.
    pub fn check_budgets(&self, layers: &Layers) -> Vec<Problem> {
        let mut problems = Vec::new();
        for (crate_name, reached) in &self.transitive {
            let Some((layer, budget)) = layers.transitive_budget_of(crate_name) else { continue };
            if reached.len() > budget {
                problems.push(Problem::OverTransitiveBudget {
                    crate_name: crate_name.clone(),
                    layer: layer.to_owned(),
                    count: reached.len(),
                    budget,
                });
            }
        }
        problems
    }

    /// One line per crate with workspace dependencies: its layer and how many
    /// crates it reaches directly and transitively, most transitive first.
    pub fn render_summary(&self, layers: &Layers) -> String {
        let mut rows = self.transitive
            .iter()
            .filter(|(_, reached)| !reached.is_empty())
            .collect::<Vec<_>>();
        rows.sort_by_key(|(name, reached)| (std::cmp::Reverse(reached.len()), *name));
        let mut text = String::from("crate | layer | direct | transitive\n");
        for (name, reached) in rows {
            let layer = layers.layer_of(name).unwrap_or("no layer");
            let direct = self.direct
                .get(name)
                .map(BTreeSet::len)
                .unwrap_or(0);
            let _ = writeln!(text, "{name} | {layer} | {direct} | {}", reached.len());
        }
        text
    }

    /// One crate's direct dependencies, and the crates it reaches only
    /// through them.
    pub fn render_crate(&self, name: &str) -> Option<String> {
        let direct = self.direct.get(name)?;
        let transitive = self.transitive.get(name)?;
        let mut text = format!(
            "{name}: {} direct and {} transitive workspace dependencies\n",
            direct.len(),
            transitive.len(),
        );
        push_list(&mut text, "Direct:", direct.iter());
        push_list(&mut text, "Through them:", transitive.difference(direct));
        Some(text)
    }
}

fn push_list<'name>(text: &mut String, heading: &str, names: impl Iterator<Item = &'name String>) {
    let _ = writeln!(text, "{heading}");
    for name in names {
        let _ = writeln!(text, "  {name}");
    }
}

fn reachable(direct: &BTreeMap<String, BTreeSet<String>>, start: &str) -> BTreeSet<String> {
    let mut reached = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(name) = pending.pop() {
        for dependency in direct.get(name).into_iter().flatten() {
            if dependency != start && reached.insert(dependency.clone()) {
                pending.push(dependency);
            }
        }
    }
    reached
}
