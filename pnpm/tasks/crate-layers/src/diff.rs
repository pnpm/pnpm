use crate::{
    graph::{CrateGraph, Edge},
    layers::Layers,
};
use std::fmt::Write as _;

/// A Markdown report of the crates and dependency edges that differ between
/// `base` and `head`, with the layers each added edge connects.
pub fn render(base: &CrateGraph, head: &CrateGraph, layers: &Layers, base_label: &str) -> String {
    let added_crates = head.crates.difference(&base.crates).collect::<Vec<_>>();
    let removed_crates = base.crates.difference(&head.crates).collect::<Vec<_>>();
    let added_edges = head.edges.difference(&base.edges).collect::<Vec<_>>();
    let removed_edges = base.edges.difference(&head.edges).collect::<Vec<_>>();

    let mut report = format!("## Crate graph changes against `{base_label}`\n\n");
    if added_crates.is_empty()
        && removed_crates.is_empty()
        && added_edges.is_empty()
        && removed_edges.is_empty()
    {
        report.push_str("No workspace crate or dependency edge changed.\n");
        return report;
    }
    for name in added_crates {
        let layer = layers.layer_of(name).unwrap_or("no layer");
        let _ = writeln!(report, "- New crate `{name}` in layer `{layer}`.");
    }
    for name in removed_crates {
        let _ = writeln!(report, "- Removed crate `{name}`.");
    }
    if !added_edges.is_empty() || !removed_edges.is_empty() {
        report.push_str("\n| Change | Edge | Kind | Layers |\n| --- | --- | --- | --- |\n");
    }
    for edge in added_edges {
        push_edge_row(&mut report, "added", edge, layers);
    }
    for edge in removed_edges {
        push_edge_row(&mut report, "removed", edge, layers);
    }
    report
}

fn push_edge_row(report: &mut String, change: &str, edge: &Edge, layers: &Layers) {
    let Edge { from, to, kind } = edge;
    let _ = writeln!(
        report,
        "| {change} | `{from}` → `{to}` | {} | {} |",
        kind.label(),
        describe_layers(edge, layers),
    );
}

fn describe_layers(edge: &Edge, layers: &Layers) -> String {
    let (Some(from), Some(to)) = (layers.layer_of(&edge.from), layers.layer_of(&edge.to)) else {
        return "unassigned".to_owned();
    };
    let direction = match layers.is_upward(edge) {
        Some(true) if !edge.kind.is_linked() => "upward, tests only",
        Some(true) if layers.is_excepted(edge) => "upward, excepted",
        Some(true) => "**upward**",
        _ if from == to => "same layer",
        _ => "downward",
    };
    format!("{from} → {to} ({direction})")
}
