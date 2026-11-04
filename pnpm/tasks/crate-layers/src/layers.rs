use crate::graph::{CrateGraph, Edge};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayersFile {
    layer: Vec<LayerEntry>,
    #[serde(default)]
    exception: Vec<Exception>,
    #[serde(default)]
    module_cycles: BTreeMap<String, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerEntry {
    name: String,
    #[expect(dead_code, reason = "documents the layer in the file; the check does not read it")]
    description: String,
    crates: Vec<String>,
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
    rank_by_crate: BTreeMap<String, usize>,
    exceptions: Vec<Exception>,
    /// The accepted size of each crate's largest module cycle.
    pub module_cycles: BTreeMap<String, usize>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    Unassigned(String),
    UnknownCrate(String),
    Upward { edge: Edge, from_layer: String, to_layer: String },
    StaleException { from: String, to: String },
    ModuleCycleGrew { crate_name: String, size: usize, baseline: usize },
    ModuleCycleShrank { crate_name: String, size: usize, baseline: usize },
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unassigned(name) => {
                write!(formatter, "{name} is in no layer; add it to crate-layers.toml")
            }
            Self::UnknownCrate(name) => {
                write!(formatter, "crate-layers.toml lists {name}, which is not a workspace crate")
            }
            Self::Upward { edge, from_layer, to_layer } => write!(
                formatter,
                "{} ({from_layer}) depends on {} ({to_layer}), a higher layer",
                edge.from, edge.to,
            ),
            Self::StaleException { from, to } => {
                write!(
                    formatter,
                    "the exception for {from} -> {to} no longer matches an upward edge; remove it",
                )
            }
            Self::ModuleCycleGrew { crate_name, size, baseline } => write!(
                formatter,
                "{crate_name} has a cycle of {size} top-level modules, more than the {baseline} \
                 in crate-layers.toml; `crate-layers modules --crate {crate_name}` lists it",
            ),
            Self::ModuleCycleShrank { crate_name, size, baseline } => write!(
                formatter,
                "{crate_name}'s largest module cycle shrank to {size}; lower its \
                 module_cycles entry in crate-layers.toml from {baseline}",
            ),
        }
    }
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
        let names = file.layer
            .into_iter()
            .map(|layer| layer.name)
            .collect();
        Ok(Self {
            names,
            rank_by_crate,
            exceptions: file.exception,
            module_cycles: file.module_cycles,
        })
    }

    pub fn layer_of(&self, name: &str) -> Option<&str> {
        self.rank_by_crate
            .get(name)
            .map(|&rank| self.names[rank].as_str())
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
