//! Semver range subset inclusion (`sub ⊆ dom`) matching strict npm `semver`
//! containment (`∀ v: satisfies(v, sub) ⇒ satisfies(v, dom)`).
//!
//! Each range is normalized into coalesced half-open intervals over release
//! `(major, minor, patch)` tuples plus coalesced half-open prerelease
//! intervals per admitted `(major, minor, patch)` tuple, so unions whose
//! branches jointly span a narrower range are recognized without re-parsing on
//! repeated checks.

use std::collections::BTreeMap;

use node_semver::{Identifier, Range};

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

struct FormattedVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre_release: Vec<Identifier>,
}

impl FormattedVersion {
    fn is_prerelease(&self) -> bool {
        !self.pre_release.is_empty()
    }

    fn tuple(&self) -> ReleaseTuple {
        (self.major, self.minor, self.patch)
    }
}

enum ElemComp {
    Any,
    Bound { op: BoundOp, version: FormattedVersion },
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
    let version = parse_formatted_version(version_str)?;
    Some(ElemComp::Bound { op, version })
}

fn parse_formatted_version(raw: &str) -> Option<FormattedVersion> {
    let without_build = raw.split('+').next()?;
    let (core, pre_str) = match without_build.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (without_build, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let pre_release = match pre_str {
        Some(pre) => pre
            .split('.')
            .map(parse_pre_identifier)
            .collect(),
        None => Vec::new(),
    };
    Some(FormattedVersion { major, minor, patch, pre_release })
}

fn parse_pre_identifier(segment: &str) -> Identifier {
    if !segment.is_empty()
        && segment.bytes().all(|byte| byte.is_ascii_digit())
        && (segment == "0" || !segment.starts_with('0'))
        && let Ok(num) = segment.parse::<u64>()
    {
        return Identifier::Numeric(num);
    }
    Identifier::AlphaNumeric(segment.to_string())
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
    version: &FormattedVersion,
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

#[derive(Default)]
struct TupleWindow {
    exclusive_floor: Option<ReleaseTuple>,
    inclusive_floor: Option<ReleaseTuple>,
    inclusive_ceiling: Option<ReleaseTuple>,
    impossible: bool,
}

impl TupleWindow {
    fn record(&mut self, op: BoundOp, tuple: ReleaseTuple, has_pre: bool) {
        match (op, has_pre) {
            (BoundOp::Eq, false) => self.impossible = true,
            (BoundOp::Eq, true) => {
                self.raise_inclusive_floor(tuple);
                self.lower_inclusive_ceiling(tuple);
            }
            (BoundOp::Gt | BoundOp::Gte, false) => self.raise_exclusive_floor(tuple),
            (BoundOp::Gt | BoundOp::Gte, true) => self.raise_inclusive_floor(tuple),
            (BoundOp::Lt | BoundOp::Lte, _) => self.lower_inclusive_ceiling(tuple),
        }
    }

    fn admits(&self, target: ReleaseTuple) -> bool {
        !self.impossible
            && self.exclusive_floor.is_none_or(|floor| target > floor)
            && self.inclusive_floor.is_none_or(|floor| target >= floor)
            && self.inclusive_ceiling.is_none_or(|ceiling| target <= ceiling)
    }

    fn raise_exclusive_floor(&mut self, tuple: ReleaseTuple) {
        self.exclusive_floor = Some(self.exclusive_floor.map_or(tuple, |prev| prev.max(tuple)));
    }

    fn raise_inclusive_floor(&mut self, tuple: ReleaseTuple) {
        self.inclusive_floor = Some(self.inclusive_floor.map_or(tuple, |prev| prev.max(tuple)));
    }

    fn lower_inclusive_ceiling(&mut self, tuple: ReleaseTuple) {
        self.inclusive_ceiling = Some(self.inclusive_ceiling.map_or(tuple, |prev| prev.min(tuple)));
    }
}

fn branch_prerelease_intervals(comps: &[ElemComp]) -> Vec<(ReleaseTuple, Interval<PrePoint>)> {
    let mut window = TupleWindow::default();
    let mut per_tuple: BTreeMap<ReleaseTuple, Vec<(BoundOp, &[Identifier])>> = BTreeMap::new();
    for comp in comps {
        if let ElemComp::Bound { op, version } = comp {
            let tuple = version.tuple();
            let has_pre = version.is_prerelease();
            window.record(*op, tuple, has_pre);
            if has_pre {
                per_tuple
                    .entry(tuple)
                    .or_default()
                    .push((*op, &version.pre_release));
            }
        }
    }
    per_tuple
        .into_iter()
        .filter(|(tuple, _)| window.admits(*tuple))
        .filter_map(|(tuple, bounds)| same_tuple_prerelease_interval(&bounds).map(|iv| (tuple, iv)))
        .collect()
}

fn same_tuple_prerelease_interval(
    bounds: &[(BoundOp, &[Identifier])],
) -> Option<Interval<PrePoint>> {
    let mut low: PrePoint = vec![Identifier::Numeric(0)];
    let mut high: Upper<PrePoint> = Upper::Unbounded;
    for &(op, pre) in bounds {
        (low, high) = narrow_same_tuple_prerelease(low, high, op, pre);
    }
    let interval = Interval { low, high };
    interval.is_non_empty().then_some(interval)
}

fn narrow_same_tuple_prerelease(
    low: PrePoint,
    high: Upper<PrePoint>,
    op: BoundOp,
    pre: &[Identifier],
) -> (PrePoint, Upper<PrePoint>) {
    let owned_pre = pre.to_vec();
    let next_pre = succ_pre(pre);
    match op {
        BoundOp::Eq => (low.max(owned_pre), high.min(Upper::Finite(next_pre))),
        BoundOp::Gte => (low.max(owned_pre), high),
        BoundOp::Gt => (low.max(next_pre), high),
        BoundOp::Lte => (low, high.min(Upper::Finite(next_pre))),
        BoundOp::Lt => (low, high.min(Upper::Finite(owned_pre))),
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

fn ends_at_or_before<Point: Ord>(high: &Upper<Point>, point: &Point) -> bool {
    match high {
        Upper::Finite(end) => end <= point,
        Upper::Unbounded => false,
    }
}

fn intervals_cover<Point: Ord>(dom: &[Interval<Point>], sub: &[Interval<Point>]) -> bool {
    let mut dom_idx = 0;
    for sub_iv in sub {
        while dom_idx < dom.len() && ends_at_or_before(&dom[dom_idx].high, &sub_iv.low) {
            dom_idx += 1;
        }
        let Some(dom_iv) = dom.get(dom_idx) else {
            return false;
        };
        if !(dom_iv.low <= sub_iv.low && sub_iv.high <= dom_iv.high) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
