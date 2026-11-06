use crate::problem::{Metric, Mismatch, Problem};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

/// Compares each crate's measured value with its accepted value in the
/// metric's table. A crate without an entry accepts `0`. A value that went
/// down fails too, so the accepted values only go down.
pub fn check(
    metric: Metric,
    measured: &BTreeMap<String, usize>,
    baseline: &BTreeMap<String, usize>,
) -> Vec<Problem> {
    let names = measured
        .keys()
        .chain(baseline.keys())
        .collect::<BTreeSet<_>>();
    let mut problems = Vec::new();
    for crate_name in names {
        let value = |table: &BTreeMap<String, usize>| {
            table
                .get(crate_name)
                .copied()
                .unwrap_or_default()
        };
        let (measured, baseline) = (value(measured), value(baseline));
        if measured != baseline {
            let crate_name = crate_name.clone();
            problems.push(Problem::Ratchet(Mismatch { metric, crate_name, measured, baseline }));
        }
    }
    problems
}

/// A table that accepts every nonzero value in `measured`.
pub fn render_baseline(metric: Metric, measured: &BTreeMap<String, usize>) -> String {
    let mut text = format!("[{}]\n", metric.table());
    for (crate_name, value) in measured
        .iter()
        .filter(|(_, value)| **value > 0)
    {
        let _ = writeln!(text, "{crate_name} = {value}");
    }
    text
}
