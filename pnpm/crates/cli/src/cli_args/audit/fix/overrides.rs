use std::collections::{BTreeMap, HashMap};

use pnpm_registry::RangeSpecStyle;

use crate::cli_args::audit::{
    AuditAdvisory, is_range_subset, min_version_from_range, patched_range_for_style,
};

/// Build the `name@vulnerable_versions → patched-range` override map from the
/// fixable advisories (those with an inferred patched range), saving each
/// minimum patched version in the style of `range_spec_style`. Keyed by a
/// `BTreeMap` so the output is sorted, mirroring pnpm's `sortDirectKeys`.
pub(crate) fn create_overrides(
    advisories: &BTreeMap<String, AuditAdvisory>,
    range_spec_style: RangeSpecStyle,
) -> BTreeMap<String, String> {
    let mut by_module: HashMap<&str, Vec<&AuditAdvisory>> = HashMap::new();
    for advisory in advisories.values() {
        if advisory.patched_versions.is_some() {
            by_module
                .entry(&advisory.module_name)
                .or_default()
                .push(advisory);
        }
    }

    let mut overrides = BTreeMap::new();
    for module_advisories in by_module.values() {
        for advisory in filter_unsubsumed_advisories(module_advisories) {
            let Some(patched) = advisory.patched_versions.as_deref() else { continue };
            let key = format!("{}@{}", advisory.module_name, advisory.vulnerable_versions);
            overrides.insert(key, patched_range_for_style(patched, range_spec_style));
        }
    }
    overrides
}

fn filter_unsubsumed_advisories<'a>(advisories: &[&'a AuditAdvisory]) -> Vec<&'a AuditAdvisory> {
    advisories
        .iter()
        .enumerate()
        .filter(|&(candidate_index, candidate)| {
            !advisories
                .iter()
                .enumerate()
                .any(|(other_index, other)| {
                    is_advisory_subsumed(candidate, candidate_index, other, other_index)
                })
        })
        .map(|(_, advisory)| *advisory)
        .collect()
}

fn is_advisory_subsumed(
    candidate: &AuditAdvisory,
    candidate_index: usize,
    other: &AuditAdvisory,
    other_index: usize,
) -> bool {
    if candidate_index == other_index {
        return false;
    }
    let (Some(candidate_patched), Some(other_patched)) =
        (&candidate.patched_versions, &other.patched_versions)
    else {
        return false;
    };
    let candidate_range = candidate.vulnerable_versions.trim();
    let other_range = other.vulnerable_versions.trim();

    if !is_range_subset(candidate_range, other_range) {
        return false;
    }

    if is_range_subset(other_range, candidate_range) {
        return break_equivalent_ranges_tie(
            candidate_patched,
            other_patched,
            candidate_index,
            other_index,
        );
    }

    true
}

fn break_equivalent_ranges_tie(
    candidate_patched: &str,
    other_patched: &str,
    candidate_index: usize,
    other_index: usize,
) -> bool {
    let candidate_min = min_version_from_range(candidate_patched);
    let other_min = min_version_from_range(other_patched);
    if let (Some(candidate_min), Some(other_min)) = (candidate_min, other_min) {
        if other_min > candidate_min {
            return true;
        }
        if other_min < candidate_min {
            return false;
        }
    }
    candidate_index > other_index
}
