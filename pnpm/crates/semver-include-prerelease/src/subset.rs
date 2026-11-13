//! Semver range subset inclusion (`sub ⊆ dom`) matching npm's `semver`.
//!
//! [`Range::allows_all`] tests whether *some* `||` branch of `sub` fits
//! inside *some* branch of `dom` and ignores prerelease tuple admission.
//! [`is_semver_subset`] checks universal inclusion across every `||`
//! branch of `sub`, enforces strict comparator parsing, and applies
//! npm's prerelease eligibility rule.

use std::borrow::Cow;

use node_semver::{Identifier, Range, Version};

use crate::{glue_operators_to_versions, npm_upper_bound};

/// Whether every version satisfying `sub` also satisfies `dom` under
/// strict npm `semver` rules (`includePrerelease: false`).
///
/// Returns `false` if either `sub` or `dom` is not a valid semver range.
#[must_use]
pub fn is_semver_subset(sub: &str, dom: &str) -> bool {
    let Some(dom_branches) = parse_union_branches(dom) else {
        return false;
    };
    let Some(sub_branches) = parse_union_branches(sub) else {
        return false;
    };
    sub_branches
        .iter()
        .all(|sub_branch| {
            matches!(sub_branch, Branch::Empty)
                || dom_branches.iter().any(|dom_branch| branch_covers(dom_branch, sub_branch))
        })
}

enum Branch {
    Empty,
    Exact(Version, Range),
    Interval(Range),
}

fn parse_union_branches(range_str: &str) -> Option<Vec<Branch>> {
    range_str
        .split("||")
        .map(parse_branch)
        .collect()
}

fn parse_branch(alternative: &str) -> Option<Branch> {
    let comparators = parse_comparator_ranges(alternative)?;
    let mut combined: Option<Range> = None;
    for comparator in comparators {
        let next = match combined {
            None => Some(comparator),
            Some(current) => current.intersect(&comparator),
        };
        let Some(range) = next else {
            return Some(Branch::Empty);
        };
        combined = Some(range);
    }
    classify_range(combined?)
}

fn parse_comparator_ranges(alternative: &str) -> Option<Vec<Range>> {
    let glued = glue_operators_to_versions(alternative);
    let tokens: Vec<&str> = glued.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(vec![Range::parse("*").ok()?]);
    }
    let mut ranges = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        let is_hyphen = tokens.get(index + 1) == Some(&"-") && index + 2 < tokens.len();
        let (range, width) = if is_hyphen {
            parse_hyphen_comparator(tokens[index], tokens[index + 2])?
        } else {
            (parse_single_comparator(tokens[index])?, 1)
        };
        ranges.push(range);
        index += width;
    }
    Some(ranges)
}

fn parse_hyphen_comparator(left: &str, right: &str) -> Option<(Range, usize)> {
    Range::parse(left).ok()?;
    Range::parse(right).ok()?;
    let range = format!("{left} - {right}").parse::<Range>().ok()?;
    Some((range, 3))
}

fn parse_single_comparator(token: &str) -> Option<Range> {
    let text = npm_upper_bound(token).map_or(Cow::Borrowed(token), Cow::Owned);
    text.parse::<Range>().ok()
}

fn classify_range(range: Range) -> Option<Branch> {
    let text = range.to_string();
    if !text.starts_with(['<', '>', '*']) {
        let version = text.parse::<Version>().ok()?;
        return Some(Branch::Exact(version, range));
    }
    if text.starts_with('<') && bound_prerelease_tuple(&text) != Some((0, 0, 0)) {
        let floor = Range::parse(">=0.0.0").ok()?;
        return Some(range.intersect(&floor).map_or(Branch::Empty, Branch::Interval));
    }
    Some(Branch::Interval(range))
}

fn branch_covers(dom: &Branch, sub: &Branch) -> bool {
    match (dom, sub) {
        (_, Branch::Empty) => true,
        (Branch::Empty, _) | (Branch::Exact(_, _), Branch::Interval(_)) => false,
        (Branch::Exact(_, dom_range) | Branch::Interval(dom_range), Branch::Exact(version, _)) => {
            dom_range.satisfies(version)
        }
        (Branch::Interval(dom_range), Branch::Interval(sub_range)) => {
            dom_range.allows_all(sub_range) && interval_prereleases_admitted(sub_range, dom_range)
        }
    }
}

fn interval_prereleases_admitted(sub_range: &Range, dom_range: &Range) -> bool {
    let sub_text = sub_range.to_string();
    let dom_text = dom_range.to_string();
    sub_text
        .split_whitespace()
        .filter_map(bound_prerelease_tuple)
        .all(|needed| {
            dom_text
                .split_whitespace()
                .filter_map(bound_prerelease_tuple)
                .any(|provided| provided == needed)
        })
}

fn bound_prerelease_tuple(token: &str) -> Option<(u64, u64, u64)> {
    let (op, version_str) = split_bound_operator(token);
    let version = version_str.parse::<Version>().ok()?;
    if !version.is_prerelease() {
        return None;
    }
    if op == "<" && version.pre_release == [Identifier::Numeric(0)] {
        return None;
    }
    Some((version.major, version.minor, version.patch))
}

fn split_bound_operator(token: &str) -> (&str, &str) {
    for op in [">=", "<=", ">", "<", "="] {
        if let Some(rest) = token.strip_prefix(op) {
            return (op, rest);
        }
    }
    ("", token)
}

#[cfg(test)]
mod tests;
