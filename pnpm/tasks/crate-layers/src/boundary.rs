use crate::{
    graph::CrateGraph,
    problem::{BoundaryViolation, Problem},
};
use std::collections::BTreeSet;

/// The product that owns a crate: the first component of its directory, such
/// as `pnpr` for `pnpr/crates/registry`.
pub fn product_of<'graph>(graph: &'graph CrateGraph, name: &str) -> Option<&'graph str> {
    graph.dirs
        .get(name)?
        .split('/')
        .find(|component| !component.is_empty() && *component != ".")
}

/// Fails on a dependency of any kind on a crate of another product that is
/// not in `shared`, and on a crate in `shared` that no crate of another
/// product depends on, so `shared` only shrinks.
pub fn check(graph: &CrateGraph, shared: &BTreeSet<String>) -> Vec<Problem> {
    let crossing = crossing_edges(graph);
    let crossed = crossing
        .iter()
        .map(|(_, to)| *to)
        .collect::<BTreeSet<_>>();
    let product = |name: &str| product_of(graph, name).unwrap_or_default().to_owned();
    let unlisted = crossing
        .iter()
        .filter(|(_, to)| !shared.contains(*to))
        .map(|(from, to)| BoundaryViolation::Unlisted {
            from: (*from).to_owned(),
            from_product: product(from),
            to: (*to).to_owned(),
            to_product: product(to),
        });
    let stale = shared
        .iter()
        .filter(|name| !crossed.contains(name.as_str()))
        .cloned()
        .map(BoundaryViolation::Stale);
    unlisted
        .chain(stale)
        .map(Problem::Boundary)
        .collect()
}

/// The `(from, to)` crate pairs, over every dependency kind, whose crates
/// belong to different products.
fn crossing_edges(graph: &CrateGraph) -> BTreeSet<(&str, &str)> {
    let crosses = |from: &str, to: &str| {
        let (from, to) = (product_of(graph, from), product_of(graph, to));
        from.is_some() && to.is_some() && from != to
    };
    graph.edges
        .iter()
        .filter(|edge| crosses(&edge.from, &edge.to))
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect()
}
