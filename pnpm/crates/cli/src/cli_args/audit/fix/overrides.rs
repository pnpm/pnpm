use std::collections::{BTreeMap, HashMap};

use pnpm_registry::RangeSpecStyle;

use crate::cli_args::audit::{
    AuditAdvisory, is_range_subset, min_version_from_range, patched_range_for_style,
};

/// Build the override map from fixable advisories, omitting entries
/// whose vulnerable ranges are subsumed by another advisory for the same package.
pub(crate) fn create_overrides(
    advisories: &BTreeMap<String, AuditAdvisory>,
    range_spec_style: RangeSpecStyle,
) -> BTreeMap<String, String> {
    let pruned = prune_subsumed_advisories(advisories);
    let mut overrides = BTreeMap::new();
    for advisory in pruned.values() {
        let Some(patched) = advisory.patched_versions.as_deref() else { continue };
        let key = format!("{}@{}", advisory.module_name, advisory.vulnerable_versions);
        overrides.insert(key, patched_range_for_style(patched, range_spec_style));
    }
    overrides
}

/// Filter out advisories whose vulnerable ranges are subsumed by another advisory
/// for the same package with an equal or greater minimum patched version.
pub(crate) fn prune_subsumed_advisories(
    advisories: &BTreeMap<String, AuditAdvisory>,
) -> BTreeMap<String, AuditAdvisory> {
    let mut by_module: HashMap<&str, Vec<(&str, &AuditAdvisory)>> = HashMap::new();
    for (id, advisory) in advisories {
        by_module
            .entry(&advisory.module_name)
            .or_default()
            .push((id.as_str(), advisory));
    }

    let mut result = BTreeMap::new();
    for module_advisories in by_module.values() {
        for (id, advisory) in filter_unsubsumed_advisories(module_advisories) {
            result.insert((*id).to_string(), (*advisory).clone());
        }
    }
    result
}

fn filter_unsubsumed_advisories<'a>(
    advisories: &[(&'a str, &'a AuditAdvisory)],
) -> Vec<(&'a str, &'a AuditAdvisory)> {
    advisories
        .iter()
        .enumerate()
        .filter(|&(candidate_index, &(_, candidate))| {
            !advisories
                .iter()
                .enumerate()
                .any(|(other_index, &(_, other))| {
                    is_advisory_subsumed(candidate, candidate_index, other, other_index)
                })
        })
        .map(|(_, item)| *item)
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
