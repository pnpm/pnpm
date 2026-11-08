use crate::graph::Edge;
use std::fmt;

/// A per-crate number that `crate-layers.toml` records and that may only go
/// down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    /// The size of the crate's largest cycle among its top-level modules.
    ModuleCycle,
    /// The number of workspace crates the crate depends on through normal
    /// and build dependencies.
    DirectDependencies,
    /// The number of workspace crates whose items the crate names in its
    /// public signatures without re-exporting them.
    LeakedDependencies,
}

impl Metric {
    pub const fn table(self) -> &'static str {
        match self {
            Self::ModuleCycle => "module_cycles",
            Self::DirectDependencies => "direct_dependencies",
            Self::LeakedDependencies => "leaked_dependencies",
        }
    }
}

/// A crate whose measured value differs from its entry in the metric's table.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mismatch {
    pub metric: Metric,
    pub crate_name: String,
    pub measured: usize,
    pub baseline: usize,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            metric,
            crate_name,
            measured,
            baseline,
        } = self;
        let table = metric.table();
        if measured < baseline {
            return write!(
                formatter,
                "{crate_name} measures {measured}, below the {baseline} in its {table} entry; \
                 lower the entry in crate-layers.toml",
            );
        }
        match metric {
            Metric::ModuleCycle => write!(
                formatter,
                "{crate_name} has a cycle of {measured} top-level modules, more than the \
                 {baseline} in crate-layers.toml; `crate-layers modules --crate {crate_name}` \
                 lists it",
            ),
            Metric::DirectDependencies => write!(
                formatter,
                "{crate_name} depends on {measured} workspace crates, more than the {baseline} \
                 in crate-layers.toml; raise its {table} entry only when the new dependency \
                 belongs in this crate",
            ),
            Metric::LeakedDependencies => write!(
                formatter,
                "{crate_name} names items of {measured} dependencies in its public signatures \
                 without re-exporting them, more than the {baseline} in crate-layers.toml; \
                 re-export the items or keep them out of the public API. \
                 `crate-layers leaks --crate {crate_name}` lists them",
            ),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    Unassigned(String),
    UnknownCrate(String),
    Upward { edge: Edge, from_layer: String, to_layer: String },
    StaleException { from: String, to: String },
    Ratchet(Mismatch),
    OverTransitiveBudget { crate_name: String, layer: String, count: usize, budget: usize },
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
            Self::Ratchet(mismatch) => mismatch.fmt(formatter),
            Self::OverTransitiveBudget { crate_name, layer, count, budget } => write!(
                formatter,
                "{crate_name} reaches {count} workspace crates through its dependencies, more \
                 than the {budget} the {layer} layer allows; `crate-layers dependencies --crate \
                 {crate_name}` lists them",
            ),
        }
    }
}
