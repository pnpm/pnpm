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
            _ => self.declared,
        };
        (range, self.packages)
    }
}

/// The intersection as `Range` prints it, desugared into comparators, with
/// each caret range written as one again and the `-0` of upper bounds
/// dropped: `>=18.18.0 <19.0.0-0` is `^18.18.0`.
fn shorten(range: &str) -> String {
    range
        .split("||")
        .map(str::trim)
        .map(shorten_set)
        .collect::<Vec<_>>()
        .join(" || ")
}

fn shorten_set(set: &str) -> String {
    let comparators: Vec<&str> = set.split_whitespace().collect();
    if let [lower, upper] = comparators.as_slice()
        && let Some(lower) = lower.strip_prefix(">=")
        && let Some(upper) = upper.strip_prefix('<')
        && let Ok(version) = Version::parse(lower)
        && version.pre_release.is_empty()
        && upper == caret_upper_bound(&version)
    {
        return format!("^{lower}");
    }
    comparators
        .iter()
        .map(|comparator| match comparator.strip_prefix('<') {
            Some(_) => comparator.strip_suffix("-0").unwrap_or(comparator),
            None => comparator,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The exclusive upper bound of `^version`, as `Range` prints it.
fn caret_upper_bound(version: &Version) -> String {
    match (version.major, version.minor) {
        (0, 0) => format!("0.0.{}-0", version.patch + 1),
        (0, minor) => format!("0.{}.0-0", minor + 1),
        (major, _) => format!("{}.0.0-0", major + 1),
    }
}
