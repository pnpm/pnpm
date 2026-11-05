use crate::{
    cycles::CycleReport,
    modules::{CrateModules, TopLevelGraph},
};
use std::{collections::BTreeMap, fmt::Write as _};

/// The module graph and largest cycle of every workspace crate.
pub fn read_all(modules: &BTreeMap<String, CrateModules>) -> Vec<(CycleReport, TopLevelGraph)> {
    modules
        .iter()
        .map(|(crate_name, modules)| {
            let graph = modules.top_level_graph();
            (CycleReport::new(crate_name, &graph), graph)
        })
        .collect()
}

/// One line per crate with a module cycle, largest cycle first.
pub fn render_summary(reports: &[(CycleReport, TopLevelGraph)]) -> String {
    let mut cyclic = reports
        .iter()
        .map(|(report, _)| report)
        .filter(|report| report.size() > 0)
        .collect::<Vec<_>>();
    cyclic.sort_by_key(|report| (std::cmp::Reverse(report.size()), report.crate_name.clone()));
    let mut text =
        String::from("crate | top-level modules | largest cycle | its share of the code\n");
    for report in cyclic {
        let share = 100 * report.largest_lines / report.total_lines.max(1);
        let _ = writeln!(
            text,
            "{} | {} | {} | {share}%",
            report.crate_name,
            report.module_count,
            report.size(),
        );
    }
    text
}

/// The modules of a crate's largest cycle and the references between them.
pub fn render_crate(report: &CycleReport, graph: &TopLevelGraph) -> String {
    let mut text = format!(
        "{}: {} top-level modules, {} code lines\n",
        report.crate_name, report.module_count, report.total_lines,
    );
    if report.largest.is_empty() {
        text.push_str("No module cycle.\n");
        return text;
    }
    let _ = writeln!(
        text,
        "Largest cycle: {} modules, {} code lines",
        report.size(),
        report.largest_lines,
    );
    for module in &report.largest {
        let lines = graph.code_lines
            .get(module)
            .copied()
            .unwrap_or_default();
        let _ = writeln!(text, "  {module} ({lines} lines)");
    }
    text.push_str("References inside the cycle (count):\n");
    let inside = |module: &String| report.largest.contains(module);
    let mut edges = graph.edges
        .iter()
        .filter(|((from, to), _)| inside(from) && inside(to))
        .collect::<Vec<_>>();
    edges.sort_by_key(|(edge, count)| (std::cmp::Reverse(**count), (*edge).clone()));
    for ((from, to), count) in edges {
        let _ = writeln!(text, "  {from} -> {to} ({count})");
    }
    text
}
