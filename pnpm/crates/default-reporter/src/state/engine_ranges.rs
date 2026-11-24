//! Merges the `engines` ranges of the packages that reject the host, so the
//! footer prints one line per set of packages that one version satisfies.

use node_semver::{
    Range,
    Version,
};
use std::collections::{
    BTreeMap,
    BTreeSet,
};

/// Packages whose ranges have a version in common.
struct Group {
    declared: String,
    /// The intersection of the group's ranges. `None` when the declared
    /// range does not parse, so the group holds only that range.
    shared: Option<Range>,
    merged: bool,
    packages: BTreeSet<String>,
}

/// Groups the packages by their declared range, adding each range to the
/// first group it has a version in common with. Each group comes with the
/// range its line prints: the declared range of a group with one range, or
/// the intersection of a merged group.
pub(super) fn merge_ranges(
    by_range: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<(String, BTreeSet<String>)> {
    let mut groups: Vec<Group> = Vec::new();
    for (declared, packages) in by_range {
        let parsed = declared.parse::<Range>().ok();
        let joined = parsed
            .as_ref()
            .and_then(|range| {
                groups
                    .iter_mut()
                    .find_map(|group| {
                        let shared = group.shared.as_ref()?.intersect(range)?;
                        Some((group, shared))
                    })
            });
        match joined {
            Some((group, shared)) => {
                group.shared = Some(shared);
                group.merged = true;
                group.packages.extend(packages.iter().cloned());
            }
            None => groups.push(Group {
                declared: declared.clone(),
                shared: parsed,
                merged: false,
                packages: packages.clone(),
            }),
        }
    }
    groups
        .into_iter()
        .map(Group::into_line)
        .collect()
}

impl Group {
    fn into_line(self) -> (String, BTreeSet<String>) {
        let range = match self.shared {
            Some(shared) if self.merged => shorten(&shared.to_string()),
            Some(shared) => {
                let short = shorten(&shared.to_string());
                if short.len() < self.declared.len() { short } else { self.declared }
            }
            None => self.declared,
        };
        (range, self.packages)
    }
}

/// One `||` set of a range: versions from `lower` (inclusive) up to `upper`
/// (exclusive). `None` leaves that side open.
struct Interval {
    lower: Option<Version>,
    upper: Option<Version>,
}

/// The range as `Range` prints it, desugared into comparators, written
/// compactly: touching or overlapping sets are joined, a caret range is
/// written as one again, and the `-0` of upper bounds is dropped.
/// `>=18.0.0 <19.0.0-0||>=19.0.0 <20.0.0-0` is `>=18.0.0 <20.0.0`.
fn shorten(range: &str) -> String {
    let sets: Vec<&str> = range
        .split("||")
        .map(str::trim)
        .collect();
    let Some(mut intervals) = sets
        .iter()
        .map(|set| parse_interval(set))
        .collect::<Option<Vec<_>>>()
    else {
        return sets.join(" || ");
    };
    intervals.sort_by(|left, right| left.lower.cmp(&right.lower));
    join_touching(intervals)
        .iter()
        .map(format_interval)
        .collect::<Vec<_>>()
        .join(" || ")
}

/// `None` for a set with a comparator other than `>=` and `<`, which
/// [`shorten`] leaves as printed.
fn parse_interval(set: &str) -> Option<Interval> {
    let mut interval = Interval { lower: None, upper: None };
    for comparator in set.split_whitespace() {
        if comparator == "*" {
            continue;
        }
        if let Some(version) = comparator.strip_prefix(">=") {
            interval.lower = Some(Version::parse(version).ok()?);
        } else if let Some(version) = comparator.strip_prefix('<')
            && !version.starts_with('=')
        {
            let version = version.strip_suffix("-0").unwrap_or(version);
            interval.upper = Some(Version::parse(version).ok()?);
        } else {
            return None;
        }
    }
    Some(interval)
}

/// Joins each interval with the previous one when they touch or overlap.
/// `intervals` is sorted by lower bound, an open one first.
fn join_touching(intervals: Vec<Interval>) -> Vec<Interval> {
    let mut joined: Vec<Interval> = Vec::new();
    for interval in intervals {
        let Some(last) = joined.last_mut() else {
            joined.push(interval);
            continue;
        };
        let touches = match (&last.upper, &interval.lower) {
            (None, _) | (_, None) => true,
            (Some(upper), Some(lower)) => lower <= upper,
        };
        if !touches {
            joined.push(interval);
            continue;
        }
        last.upper = match (last.upper.take(), interval.upper) {
            (Some(left), Some(right)) => Some(left.max(right)),
            _ => None,
        };
    }
    joined
}

fn format_interval(interval: &Interval) -> String {
    match (&interval.lower, &interval.upper) {
        (Some(lower), Some(upper)) if is_caret(lower, upper) => format!("^{lower}"),
        (Some(lower), Some(upper)) => format!(">={lower} <{upper}"),
        (Some(lower), None) => format!(">={lower}"),
        (None, Some(upper)) => format!("<{upper}"),
        (None, None) => "*".to_string(),
    }
}

/// Whether `[lower, upper)` is `^lower`.
fn is_caret(lower: &Version, upper: &Version) -> bool {
    let bound = match (lower.major, lower.minor) {
        (0, 0) => (0, 0, lower.patch + 1),
        (0, minor) => (0, minor + 1, 0),
        (major, _) => (major + 1, 0, 0),
    };
    lower.pre_release.is_empty()
        && upper.pre_release.is_empty()
        && (upper.major, upper.minor, upper.patch) == bound
}
