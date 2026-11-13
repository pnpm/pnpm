//! Semver range subset inclusion (`sub ⊆ dom`) matching strict npm `semver`
//! containment (`∀ v: satisfies(v, sub) ⇒ satisfies(v, dom)`).
//!
//! Each range is normalized into coalesced half-open intervals over release
//! `(major, minor, patch)` tuples plus coalesced half-open prerelease
//! intervals per admitted `(major, minor, patch)` tuple, so unions whose
//! branches jointly span a subrange (such as `^1.0.0 || ^2.0.0` covering
//! `>=1.5.0 <2.5.0`) are recognized without re-parsing on repeated checks.

use std::collections::{BTreeMap, BTreeSet};

use node_semver::{Identifier, Range, Version};

use crate::{glue_operators_to_versions, npm_upper_bound};

type ReleaseTuple = (u64, u64, u64);
type ReleasePoint = (u64, u64, u128);
type PrePoint = Vec<Identifier>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Upper<Point> {
    Finite(Point),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval<Point> {
    low: Point,
    high: Upper<Point>,
}

impl<Point: Ord> Interval<Point> {
    fn is_non_empty(&self) -> bool {
        match &self.high {
            Upper::Unbounded => true,
            Upper::Finite(high) => self.low < *high,
        }
    }
}

/// Pre-parsed semver range normalized into coalesced release and prerelease
/// intervals under strict npm `semver` rules (`includePrerelease: false`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSemverRange {
    release_intervals: Vec<Interval<ReleasePoint>>,
    prerelease_intervals: BTreeMap<ReleaseTuple, Vec<Interval<PrePoint>>>,
}

impl ParsedSemverRange {
    /// Parses `input` as a strict npm `semver` range, returning `None` if any
    /// comparator in any `||` branch is invalid.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let mut release_raw = Vec::new();
        let mut prerelease_raw: BTreeMap<ReleaseTuple, Vec<Interval<PrePoint>>> = BTreeMap::new();
        for alternative in input.split("||") {
            let comps = parse_branch_comparators(alternative)?;
            if let Some(release_iv) = branch_release_interval(&comps) {
                release_raw.push(release_iv);
            }
            for (tuple, pre_iv) in branch_prerelease_intervals(&comps) {
                prerelease_raw
                    .entry(tuple)
                    .or_default()
                    .push(pre_iv);
            }
        }
        let release_intervals = coalesce_intervals(release_raw);
        let prerelease_intervals = prerelease_raw
            .into_iter()
            .map(|(tuple, ivs)| (tuple, coalesce_intervals(ivs)))
            .filter(|(_, ivs)| !ivs.is_empty())
            .collect();
        Some(Self { release_intervals, prerelease_intervals })
    }

    /// Whether every version satisfying `sub` also satisfies `self`.
    #[must_use]
    pub fn covers(&self, sub: &Self) -> bool {
        intervals_cover(&self.release_intervals, &sub.release_intervals)
            && sub.prerelease_intervals
                .iter()
                .all(|(tuple, sub_ivs)| {
                    self.prerelease_intervals
                        .get(tuple)
                        .is_some_and(|dom_ivs| intervals_cover(dom_ivs, sub_ivs))
                })
    }
}

/// Whether every version satisfying `sub` also satisfies `dom` under
/// strict npm `semver` rules (`includePrerelease: false`).
///
/// Returns `false` if either `sub` or `dom` is not a valid semver range.
#[must_use]
pub fn is_semver_subset(sub: &str, dom: &str) -> bool {
    let Some(parsed_dom) = ParsedSemverRange::parse(dom) else {
        return false;
    };
    let Some(parsed_sub) = ParsedSemverRange::parse(sub) else {
        return false;
    };
    parsed_dom.covers(&parsed_sub)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

enum ElemComp {
    Any,
    Bound { op: BoundOp, version: Version },
}

fn parse_branch_comparators(alternative: &str) -> Option<Vec<ElemComp>> {
    let ranges = parse_comparator_ranges(alternative)?;
    let mut elems = Vec::with_capacity(ranges.len() * 2);
    for range in ranges {
        let formatted = range.to_string();
        for token in formatted.split_whitespace() {
            elems.push(parse_formatted_bound(token)?);
        }
    }
    Some(elems)
}

fn parse_formatted_bound(token: &str) -> Option<ElemComp> {
    if token == "*" {
        return Some(ElemComp::Any);
    }
    let (op, version_str) = split_bound_operator(token);
    let version = version_str.parse::<Version>().ok()?;
    Some(ElemComp::Bound { op, version })
}

fn split_bound_operator(token: &str) -> (BoundOp, &str) {
    for (prefix, op) in [
        (">=", BoundOp::Gte),
        ("<=", BoundOp::Lte),
        (">", BoundOp::Gt),
        ("<", BoundOp::Lt),
        ("=", BoundOp::Eq),
    ] {
        if let Some(rest) = token.strip_prefix(prefix) {
            return (op, rest);
        }
    }
    (BoundOp::Eq, token)
}

fn parse_comparator_ranges(alternative: &str) -> Option<Vec<Range>> {
    let glued = glue_operators_to_versions(alternative);
    let tokens: Vec<&str> = glued.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(vec![Range::any()]);
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
    let parsed = token.parse::<Range>().ok()?;
    match npm_upper_bound(token) {
        Some(rewritten) => rewritten.parse::<Range>().ok(),
        None => Some(parsed),
    }
}

fn branch_release_interval(comps: &[ElemComp]) -> Option<Interval<ReleasePoint>> {
    let mut low: ReleasePoint = (0, 0, 0);
    let mut high: Upper<ReleasePoint> = Upper::Unbounded;
    for comp in comps {
        if let ElemComp::Bound { op, version } = comp {
            (low, high) = apply_release_bound(low, high, *op, version)?;
        }
    }
    let interval = Interval { low, high };
    interval.is_non_empty().then_some(interval)
}

fn apply_release_bound(
    low: ReleasePoint,
    high: Upper<ReleasePoint>,
    op: BoundOp,
    version: &Version,
) -> Option<(ReleasePoint, Upper<ReleasePoint>)> {
    let point: ReleasePoint = (version.major, version.minor, u128::from(version.patch));
    let next_point: ReleasePoint = (version.major, version.minor, u128::from(version.patch) + 1);
    let has_pre = version.is_prerelease();
    match (op, has_pre) {
        (BoundOp::Eq, true) => None,
        (BoundOp::Eq, false) => Some((low.max(point), high.min(Upper::Finite(next_point)))),
        (BoundOp::Gte, _) | (BoundOp::Gt, true) => Some((low.max(point), high)),
        (BoundOp::Gt, false) => Some((low.max(next_point), high)),
        (BoundOp::Lte, false) => Some((low, high.min(Upper::Finite(next_point)))),
        (BoundOp::Lt, _) | (BoundOp::Lte, true) => Some((low, high.min(Upper::Finite(point)))),
    }
}

fn branch_prerelease_intervals(comps: &[ElemComp]) -> Vec<(ReleaseTuple, Interval<PrePoint>)> {
    let candidate_tuples: BTreeSet<ReleaseTuple> = comps
        .iter()
        .filter_map(|comp| match comp {
            ElemComp::Bound { version, .. } if version.is_prerelease() => {
                Some((version.major, version.minor, version.patch))
            }
            _ => None,
        })
        .collect();
    candidate_tuples
        .into_iter()
        .filter_map(|tuple| tuple_prerelease_interval(tuple, comps).map(|iv| (tuple, iv)))
        .collect()
}

fn tuple_prerelease_interval(
    tuple: ReleaseTuple,
    comps: &[ElemComp],
) -> Option<Interval<PrePoint>> {
    let mut low: PrePoint = vec![Identifier::Numeric(0)];
    let mut high: Upper<PrePoint> = Upper::Unbounded;
    for comp in comps {
        if let ElemComp::Bound { op, version } = comp {
            (low, high) = apply_prerelease_bound(tuple, low, high, *op, version)?;
        }
    }
    let interval = Interval { low, high };
    interval.is_non_empty().then_some(interval)
}

fn apply_prerelease_bound(
    target: ReleaseTuple,
    low: PrePoint,
    high: Upper<PrePoint>,
    op: BoundOp,
    version: &Version,
) -> Option<(PrePoint, Upper<PrePoint>)> {
    let comp_tuple = (version.major, version.minor, version.patch);
    if comp_tuple != target || !version.is_prerelease() {
        return admits_non_matching_tuple(target, comp_tuple, op).then_some((low, high));
    }
    let pre = version.pre_release.clone();
    let next_pre = succ_pre(&pre);
    let updated = match op {
        BoundOp::Eq => (low.max(pre), high.min(Upper::Finite(next_pre))),
        BoundOp::Gte => (low.max(pre), high),
        BoundOp::Gt => (low.max(next_pre), high),
        BoundOp::Lte => (low, high.min(Upper::Finite(next_pre))),
        BoundOp::Lt => (low, high.min(Upper::Finite(pre))),
    };
    Some(updated)
}

fn admits_non_matching_tuple(target: ReleaseTuple, comp_tuple: ReleaseTuple, op: BoundOp) -> bool {
    if comp_tuple < target {
        matches!(op, BoundOp::Gt | BoundOp::Gte)
    } else {
        matches!(op, BoundOp::Lt | BoundOp::Lte)
    }
}

fn succ_pre(pre: &[Identifier]) -> PrePoint {
    let mut next = Vec::with_capacity(pre.len() + 1);
    next.extend_from_slice(pre);
    next.push(Identifier::Numeric(0));
    next
}

fn coalesce_intervals<Point: Ord + Clone>(
    mut intervals: Vec<Interval<Point>>,
) -> Vec<Interval<Point>> {
    intervals.sort_by(|lhs, rhs| lhs.low.cmp(&rhs.low));
    let mut merged: Vec<Interval<Point>> = Vec::with_capacity(intervals.len());
    for next in intervals {
        if let Some(current) = merged.last_mut()
            && Upper::Finite(next.low.clone()) <= current.high
        {
            if next.high > current.high {
                current.high = next.high;
            }
        } else {
            merged.push(next);
        }
    }
    merged
}

fn intervals_cover<Point: Ord>(dom: &[Interval<Point>], sub: &[Interval<Point>]) -> bool {
    sub.iter()
        .all(|sub_iv| {
            dom.iter()
                .any(|dom_iv| dom_iv.low <= sub_iv.low && sub_iv.high <= dom_iv.high)
        })
}

#[cfg(test)]
mod tests;
