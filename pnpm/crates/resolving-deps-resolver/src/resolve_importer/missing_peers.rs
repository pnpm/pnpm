use super::{BTreeMap, BTreeSet, HashMap, HashSet, MissingPeer, MissingPeerInfo, Range};

/// Split the missing-peer report into the inputs the inner and outer
/// loops consume.
///
/// A peer name is **required** for this iteration when at least one of
/// its consumers declared it non-optional and it isn't already in
/// `parent_pkg_aliases` (i.e. not already a direct dep that just
/// hadn't been added to the alias set yet). Its merged range is
/// computed by [`merge_ranges`].
///
/// Peers whose consumers are *all* optional are returned as the second
/// component, keyed by peer name with the deduplicated range list the
/// outer loop's [`fn@crate::get_hoistable_optional_peers`] needs.
pub(super) fn partition_missing_peers(
    missing: &HashMap<String, Vec<MissingPeer>>,
    parent_pkg_aliases: &HashSet<String>,
    auto_install_peers_from_highest_match: bool,
) -> (BTreeMap<String, MissingPeerInfo>, BTreeMap<String, Vec<String>>) {
    let mut missing_required: BTreeMap<String, MissingPeerInfo> = BTreeMap::new();
    let mut missing_optional: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (peer_name, entries) in missing {
        if parent_pkg_aliases.contains(peer_name) {
            continue;
        }
        match classify_missing_peer(entries, auto_install_peers_from_highest_match) {
            MissingPeerKind::Required(range) => {
                missing_required.insert(peer_name.clone(), MissingPeerInfo { range });
            }
            MissingPeerKind::Optional(ranges) => {
                missing_optional.insert(peer_name.clone(), ranges);
            }
            MissingPeerKind::Unhoistable => {}
        }
    }
    (missing_required, missing_optional)
}

/// What the hoist can do with one missing peer's recorded requirements.
pub(super) enum MissingPeerKind {
    Required(String),
    Optional(Vec<String>),
    /// Nothing to install: no range survived the merge, or every entry was
    /// optional and named no range.
    Unhoistable,
}

pub(super) fn classify_missing_peer(
    entries: &[MissingPeer],
    auto_install_peers_from_highest_match: bool,
) -> MissingPeerKind {
    // Hoisting a missing required peer fetches it, so it needs the original
    // specifier with its scheme preserved (`work:5.x.x`); hoist_peers reduces
    // it to a comparable range itself. The optional path below dedupes onto
    // an already-present version, so it uses the display range instead.
    let required_ranges: Vec<&str> = entries
        .iter()
        .filter(|entry| !entry.optional)
        .map(|entry| entry.raw_range.as_str())
        .collect();
    if required_ranges.is_empty() {
        let ordered = distinct_wanted_ranges(entries);
        if ordered.is_empty() {
            return MissingPeerKind::Unhoistable;
        }
        return MissingPeerKind::Optional(ordered);
    }
    match merge_ranges(&required_ranges, auto_install_peers_from_highest_match) {
        Some(range) => MissingPeerKind::Required(range),
        None => MissingPeerKind::Unhoistable,
    }
}

/// The distinct wanted ranges the entries name, in first-seen order.
pub(super) fn distinct_wanted_ranges(entries: &[MissingPeer]) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut ordered: Vec<String> = Vec::new();
    for entry in entries {
        if seen.insert(entry.wanted_range.clone()) {
            ordered.push(entry.wanted_range.clone());
        }
    }
    ordered
}

/// Combine multiple consumers' wanted ranges into a single specifier,
/// the upstream `mergePkgsDeps` policy: a single (or single unique)
/// range passes through unchanged, distinct compatible ranges reduce to
/// their semver intersection, and an empty intersection falls back to a
/// `||`-join under `auto_install_peers_from_highest_match` — or `None`,
/// dropping the peer on an unresolvable conflict.
///
/// Only the unique ranges reach [`intersect_ranges`]: folding a range in
/// a second time cannot narrow the result, and every pass costs another
/// Cartesian product over the alternatives.
pub(super) fn merge_ranges(
    ranges: &[&str],
    auto_install_peers_from_highest_match: bool,
) -> Option<String> {
    if ranges.len() == 1 {
        return Some(ranges[0].to_string());
    }
    let mut seen: HashSet<&str> = HashSet::default();
    let unique: Vec<&str> = ranges.iter().copied().filter(|&range| seen.insert(range)).collect();
    if unique.len() == 1 {
        return Some(ranges[0].to_string());
    }
    if let Some(intersection) = intersect_ranges(&unique) {
        return Some(intersection);
    }
    if auto_install_peers_from_highest_match {
        return Some(ranges.join(" || "));
    }
    None
}

/// Semver intersection of every range, rendered in `node-semver`'s
/// canonical form (`2` ∩ `^2.2.0` → `>=2.2.0 <3.0.0-0`, the same shape
/// the `semver-range-intersect` npm package emits upstream). `None`
/// when a range fails to parse or the ranges share no versions,
/// mirroring `safeIntersect`'s caught-throw `null`.
pub(super) fn intersect_ranges(ranges: &[&str]) -> Option<String> {
    let mut iter = ranges.iter();
    let first = Range::parse(iter.next()?).ok()?;
    iter.try_fold(first, |acc, range| {
        Range::parse(range)
            .ok()
            .and_then(|range| acc.intersect(&range))
            .map(|intersection| collapse_covered_alternatives(&intersection))
    })
    .map(|range| range.to_string())
}

/// Drop the alternatives of a union that another alternative already
/// covers, leaving the same set of versions behind.
///
/// [`Range::intersect`] pairs every alternative of one union with every
/// alternative of the other, so an alternative two consumers agree on
/// survives once per pair and the next intersection multiplies that
/// count again. `semver-range-intersect` collapses the union upstream,
/// which is why the growth is pacquet's alone.
pub(super) fn collapse_covered_alternatives(range: &Range) -> Range {
    let rendered = range.to_string();
    let alternatives: Vec<&str> = rendered.split("||").collect();
    if alternatives.len() < 2 {
        return range.clone();
    }
    let Some(parsed) = alternatives
        .iter()
        .map(|alternative| Range::parse(alternative).ok())
        .collect::<Option<Vec<Range>>>()
    else {
        return range.clone();
    };
    let mut kept: Vec<&str> = Vec::with_capacity(alternatives.len());
    for (index, alternative) in parsed.iter().enumerate() {
        // Of two alternatives that cover each other, only the first is kept.
        let covered = parsed.iter().enumerate().any(|(other_index, other)| {
            other_index != index
                && other.allows_all(alternative)
                && (other_index < index || !alternative.allows_all(other))
        });
        if !covered {
            kept.push(alternatives[index]);
        }
    }
    Range::parse(kept.join("||")).unwrap_or_else(|_| range.clone())
}
