//! Semver questions the audit asks of advisory ranges.

use super::{Range, Version};

pub(crate) fn satisfies_safe(version: &str, range: &str) -> bool {
    let Ok(version) = version.parse::<Version>() else { return false };
    let Ok(range) = range.parse::<Range>() else { return false };
    satisfies_including_prerelease(&version, &range)
}

pub(crate) fn satisfies_including_prerelease(version: &Version, range: &Range) -> bool {
    if version.satisfies(range) {
        return true;
    }
    range.to_string().split("||").any(|comparators| {
        comparators.split_whitespace().all(|comparator| comparator_matches(version, comparator))
    })
}

pub(crate) fn comparator_matches(version: &Version, comparator: &str) -> bool {
    if comparator == "*" {
        return true;
    }
    let (operator, wanted) = comparator_operator_and_version(comparator);
    let Ok(wanted) = wanted.parse::<Version>() else { return false };
    match operator {
        ">" => version > &wanted,
        ">=" => version >= &wanted,
        "<" => version < &wanted,
        "<=" => version <= &wanted,
        _ => version == &wanted,
    }
}

pub(crate) fn comparator_operator_and_version(comparator: &str) -> (&str, &str) {
    for operator in [">=", "<=", ">", "<"] {
        if let Some(version) = comparator.strip_prefix(operator) {
            return (operator, version);
        }
    }
    ("", comparator)
}

pub(crate) fn infer_patched_versions(vulnerable_range: &str) -> Option<String> {
    let (operator, version) = last_upper_bound(vulnerable_range.trim())?;
    let version = version.parse::<Version>().ok()?;
    match operator {
        "<" => Some(format!(">={version}")),
        "<=" => {
            let next = Version {
                major: version.major,
                minor: version.minor,
                patch: version.patch + 1,
                pre_release: Vec::new(),
                build: Vec::new(),
            };
            Some(format!(">={next}"))
        }
        _ => None,
    }
}

pub(crate) fn last_upper_bound(input: &str) -> Option<(&str, &str)> {
    let mut parts = input.split_whitespace().collect::<Vec<_>>();
    let last = parts.pop()?;
    if let Some(version) = last.strip_prefix("<=") {
        return Some(("<=", version.trim()));
    }
    if let Some(version) = last.strip_prefix('<') {
        return Some(("<", version.trim()));
    }
    let operator = parts.pop()?;
    matches!(operator, "<" | "<=").then_some((operator, last))
}

/// The minimum patched version with a caret, mirroring pnpm's
/// `caretRangeForPatched`: `^X.Y.Z` keeps the resolver within the same major
/// the user pinned to, where a bare `>=X.Y.Z` could silently promote a dep to
/// a later breaking major. `patched` is always pacquet's inferred `>=V` form,
/// so its minimum is the version after `>=`.
pub(crate) fn caret_range_for_patched(patched: &str) -> String {
    patched
        .strip_prefix(">=")
        .and_then(|version| version.trim().parse::<Version>().ok())
        .map_or_else(|| patched.to_string(), |version| format!("^{version}"))
}
