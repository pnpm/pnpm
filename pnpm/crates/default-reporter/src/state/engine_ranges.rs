//! Merges the `engines` ranges of the packages that reject the host, so the
//! footer prints one line per set of packages that one version satisfies.

use node_semver::{
    Range,
    Version,
};
use pnpm_semver_include_prerelease::IncludePrereleaseRange;
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
/// the intersection of a merged group. A printed range never accepts
/// `current`, the version the packages reject.
pub(super) fn merge_ranges(
    by_range: &BTreeMap<String, BTreeSet<String>>,
    current: &str,
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
    let current = Version::parse(current).ok();
    groups
        .into_iter()
        .map(|group| group.into_line(current.as_ref()))
        .collect()
}

impl Group {
    /// The compact form of the range when it still rejects `current`. Both
    /// [`shorten`] and [`Range`] print an upper bound such as `<26` as
    /// `<26.0.0`, which accepts the prerelease `26.0.0-rc.1` that the
    /// declared range rejects. Such a range prints its upper bounds with
    /// `-0`.
    fn into_line(self, current: Option<&Version>) -> (String, BTreeSet<String>) {
        let rejects_current = |range: &String| {
            current.is_none_or(|current| !IncludePrereleaseRange::parse(range).satisfies(current))
        };
        let range = match &self.shared {
            Some(shared) => {
                let printed = shared.to_string();
                let exact = if self.merged {
                    printed
                        .split("||")
                        .collect::<Vec<_>>()
                        .join(" || ")
                } else {
                    self.declared
                };
                [Bounds::Release, Bounds::Prerelease]
                    .into_iter()
                    .map(|bounds| shorten(&printed, bounds))
                    .filter(|short| self.merged || short.len() < exact.len())
                    .find(rejects_current)
                    .unwrap_or(exact)
            }
            None => self.declared,
        };
        (range, self.packages)
    }
}

/// How [`shorten`] prints an upper bound that is not part of a caret range.
#[derive(Clone, Copy)]
enum Bounds {
    /// `<26.0.0`.
    Release,
    /// `<26.0.0-0`, which also rejects the prereleases of `26.0.0`.
    Prerelease,
}

/// One `||` set of a range: versions from `lower` (inclusive) up to `upper`
/// (exclusive). `None` leaves that side open.
struct Interval {
    lower: Option<Version>,
    upper: Option<UpperBound>,
}

struct UpperBound {
    version: Version,
    /// Whether the bound was printed with `-0`, so it also excludes the
    /// prereleases of `version`, as the bound of a caret range does.
    excludes_prereleases: bool,
}

/// The range as [`Range`] prints it, desugared into comparators, written
/// compactly: touching or overlapping sets are joined, a caret range is
/// written as one again, and the `-0` of upper bounds is dropped.
/// `>=18.0.0 <19.0.0-0||>=19.0.0 <20.0.0-0` is `>=18.0.0 <20.0.0`.
fn shorten(range: &str, bounds: Bounds) -> String {
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
        .map(|interval| format_interval(interval, bounds))
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
            let stripped = version.strip_suffix("-0");
            interval.upper = Some(UpperBound {
                version: Version::parse(stripped.unwrap_or(version)).ok()?,
                excludes_prereleases: stripped.is_some(),
            });
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
            (Some(upper), Some(lower)) => *lower <= upper.version,
        };
        if !touches {
            joined.push(interval);
            continue;
        }
        last.upper = match (last.upper.take(), interval.upper) {
            (Some(left), Some(right)) if left.version >= right.version => Some(left),
            (Some(_), Some(right)) => Some(right),
            _ => None,
        };
    }
    joined
}

fn format_interval(interval: &Interval, bounds: Bounds) -> String {
    let upper = |upper: &UpperBound| match bounds {
        Bounds::Prerelease if upper.version.pre_release.is_empty() => {
            format!("{}-0", upper.version)
        }
        Bounds::Release | Bounds::Prerelease => upper.version.to_string(),
    };
    match (&interval.lower, &interval.upper) {
        (Some(lower), Some(bound)) if is_caret(lower, bound) => format!("^{lower}"),
        (Some(lower), Some(bound)) => format!(">={lower} <{}", upper(bound)),
        (Some(lower), None) => format!(">={lower}"),
        (None, Some(bound)) => format!("<{}", upper(bound)),
        (None, None) => "*".to_string(),
    }
}

/// Whether `[lower, upper)` is `^lower`.
fn is_caret(lower: &Version, upper: &UpperBound) -> bool {
    let bound = match (lower.major, lower.minor) {
        (0, 0) => (0, 0, lower.patch + 1),
        (0, minor) => (0, minor + 1, 0),
        (major, _) => (major + 1, 0, 0),
    };
    let upper_version = &upper.version;
    upper.excludes_prereleases
        && lower.pre_release.is_empty()
        && upper_version.pre_release.is_empty()
        && (upper_version.major, upper_version.minor, upper_version.patch) == bound
}
