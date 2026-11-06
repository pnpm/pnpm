use crate::{
    graph::{CrateGraph, Edge},
    problem::{Metric, Problem},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayersFile {
    layer: Vec<LayerEntry>,
    #[serde(default)]
    exception: Vec<Exception>,
    #[serde(default)]
    module_cycles: BTreeMap<String, usize>,
    #[serde(default)]
    direct_dependencies: BTreeMap<String, usize>,
    #[serde(default)]
    leaked_dependencies: BTreeMap<String, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerEntry {
    name: String,
    #[expect(dead_code, reason = "documents the layer in the file; the check does not read it")]
    description: String,
    crates: Vec<String>,
    max_transitive_dependencies: Option<usize>,
}

/// An upward edge accepted until the refactor that removes it lands.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub from: String,
    pub to: String,
    #[expect(
        dead_code,
        reason = "documents the exception in the file; the check does not read it"
    )]
    reason: String,
}

/// The layer assignment from `crate-layers.toml`, bottom layer first.
#[derive(Debug)]
pub struct Layers {
    names: Vec<String>,
    /// Each layer's `max_transitive_dependencies`, by rank.
    transitive_budgets: Vec<Option<usize>>,
    rank_by_crate: BTreeMap<String, usize>,
    exceptions: Vec<Exception>,
    baselines: BTreeMap<Metric, BTreeMap<String, usize>>,
}

impl Layers {
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: LayersFile =
            toml::from_str(text).map_err(|error| format!("parse crate-layers.toml: {error}"))?;
        let mut rank_by_crate = BTreeMap::new();
        for (rank, layer) in file.layer.iter().enumerate() {
            for name in &layer.crates {
                if rank_by_crate.insert(name.clone(), rank).is_some() {
                    return Err(format!("crate-layers.toml puts {name} in more than one layer"));
                }
            }
        }
        let transitive_budgets = file.layer
            .iter()
            .map(|layer| layer.max_transitive_dependencies)
            .collect();
        let names = file.layer
            .into_iter()
            .map(|layer| layer.name)
            .collect();
        Ok(Self {
            names,
            transitive_budgets,
            rank_by_crate,
            exceptions: file.exception,
            baselines: BTreeMap::from([
                (Metric::ModuleCycle, file.module_cycles),
                (Metric::DirectDependencies, file.direct_dependencies),
                (Metric::LeakedDependencies, file.leaked_dependencies),
            ]),
        })
    }

    /// The accepted value of each crate in the metric's table.
    pub fn baseline(&self, metric: Metric) -> &BTreeMap<String, usize> {
        &self.baselines[&metric]
    }

    pub fn layer_of(&self, name: &str) -> Option<&str> {
        self.rank_by_crate
            .get(name)
            .map(|&rank| self.names[rank].as_str())
    }

    /// The crate's layer and that layer's `max_transitive_dependencies`, when it sets one.
    pub fn transitive_budget_of(&self, name: &str) -> Option<(&str, usize)> {
        let rank = *self.rank_by_crate.get(name)?;
        Some((self.names[rank].as_str(), self.transitive_budgets[rank]?))
    }

    /// Whether `edge` points to a higher layer. `None` when either end is unassigned.
    pub fn is_upward(&self, edge: &Edge) -> Option<bool> {
        Some(self.rank_by_crate.get(&edge.to)? > self.rank_by_crate.get(&edge.from)?)
    }

    pub fn is_excepted(&self, edge: &Edge) -> bool {
        self.exceptions
            .iter()
            .any(|exception| exception.from == edge.from && exception.to == edge.to)
    }

    pub fn check(&self, graph: &CrateGraph) -> BTreeSet<Problem> {
        let mut problems = BTreeSet::new();
        let unassigned = graph.crates
            .iter()
            .filter(|name| !self.rank_by_crate.contains_key(*name));
        problems.extend(unassigned.cloned().map(Problem::Unassigned));
        let unknown = self.rank_by_crate
            .keys()
            .filter(|name| !graph.crates.contains(*name));
        problems.extend(unknown.cloned().map(Problem::UnknownCrate));
        let upward = self.upward_edges(graph);
        let violations = upward
            .iter()
            .filter(|edge| !self.is_excepted(edge));
        problems.extend(violations.map(|edge| self.upward_problem(edge)));
        problems.extend(self.stale_exceptions(&upward));
        problems
    }

    fn upward_edges<'graph>(&self, graph: &'graph CrateGraph) -> Vec<&'graph Edge> {
        let linked = graph.edges.iter().filter(|edge| edge.kind.is_linked());
        linked
            .filter(|edge| self.is_upward(edge) == Some(true))
            .collect()
    }

    fn upward_problem(&self, edge: &Edge) -> Problem {
        let layer = |name: &str| {
            self.layer_of(name)
                .unwrap_or_default()
                .to_owned()
        };
        Problem::Upward {
            edge: edge.clone(),
            from_layer: layer(&edge.from),
            to_layer: layer(&edge.to),
        }
    }

    fn stale_exceptions(&self, upward: &[&Edge]) -> Vec<Problem> {
        let matches = |exception: &Exception| {
            upward
                .iter()
                .any(|edge| edge.from == exception.from && edge.to == exception.to)
        };
        let stale = self.exceptions
            .iter()
            .filter(|exception| !matches(exception));
        stale
            .map(|exception| Problem::StaleException {
                from: exception.from.clone(),
                to: exception.to.clone(),
            })
            .collect()
    }
}
