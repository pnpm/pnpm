use super::{
    BTreeMap, Package, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
    semver_satisfies_loose,
};

/// An exact preferred version that satisfies the range but is absent from
/// `meta` can only be learned from the registry.
pub(crate) fn cached_meta_misses_preferred_version(
    meta: &Package,
    version_range: &str,
    preferred_version_selectors: Option<&VersionSelectors>,
) -> bool {
    let Some(selectors) = preferred_version_selectors else {
        return false;
    };
    selectors
        .iter()
        .any(|(selector, entry)| {
            if selector == version_range {
                return false;
            }
            let (selector_type, _) = selector_info(entry);
            selector_type == VersionSelectorType::Version
                && semver_satisfies_loose(selector, version_range)
                && !meta.versions.contains_key(selector)
        })
}

/// The exact selector a pick for `version_range` is proven to land on,
/// whatever the registry publishes after the packument was cached. Each
/// exact selector that satisfies the range is guaranteed its own weight plus
/// every preferred range it satisfies; an out-of-range one is discarded by
/// max/min satisfying. The heaviest must exceed what the range and tag
/// selectors could give any other version, and every other exact selector
/// must stay below it even with every tag's weight added, so a tie never
/// qualifies. `None` when no exact selector satisfies the range, a selector
/// is zero-weighted (the prioritizer's replaceable seed sentinel), or the
/// proof does not hold.
pub(crate) fn dominant_lockfile_version(
    version_range: &str,
    preferred_version_selectors: Option<&VersionSelectors>,
) -> Option<String> {
    let (mut exact, mut ranges, mut tags) = (Vec::new(), Vec::new(), 0_u64);
    for (selector, entry) in preferred_version_selectors? {
        match selector_info(entry) {
            _ if selector == version_range => {}
            (_, 0) => return None,
            (VersionSelectorType::Version, weight)
                if semver_satisfies_loose(selector, version_range) =>
            {
                exact.push((selector, u64::from(weight)));
            }
            (VersionSelectorType::Version, _) => {}
            (VersionSelectorType::Range, weight) => ranges.push((selector, u64::from(weight))),
            (VersionSelectorType::Tag, weight) => tags += u64::from(weight),
        }
    }
    for (version, weight) in &mut exact {
        *weight += ranges
            .iter()
            .filter(|range| semver_satisfies_loose(version, range.0))
            .map(|range| range.1)
            .sum::<u64>();
    }
    let movable = tags
        + ranges
            .iter()
            .map(|range| range.1)
            .sum::<u64>();
    let &(winner, top) = exact.iter().max_by_key(|(_, weight)| *weight)?;
    let others_stay_below = exact
        .iter()
        .all(|&(version, weight)| version == winner || weight + tags < top);
    (top > movable && others_stay_below).then(|| winner.clone())
}

pub(super) fn selector_info(entry: &VersionSelectorEntry) -> (VersionSelectorType, u32) {
    match entry {
        VersionSelectorEntry::Plain(selector_type) => (*selector_type, 1),
        VersionSelectorEntry::Weighted(weighted) => (weighted.selector_type, weighted.weight),
    }
}

/// Group versions by weight (highest weight first); each group is
/// the input to a single max/min-satisfying call.
pub(super) fn prioritize_preferred_versions(
    meta: &Package,
    version_range: &str,
    preferred_version_selectors: Option<&VersionSelectors>,
) -> Vec<Vec<String>> {
    let mut prioritizer = PreferredVersionsPrioritizer::default();

    // Seed every range-satisfying version at weight 0. JS treats 0
    // as falsy, so a later positive-weight `add` overwrites this
    // sentinel rather than summing with it — preserved below in
    // [`PreferredVersionsPrioritizer::add`].
    for version in meta.versions.keys() {
        if semver_satisfies_loose(version, version_range) {
            prioritizer.add(version.clone(), 0);
        }
    }

    for (preferred_selector, entry) in preferred_version_selectors.into_iter().flatten() {
        if preferred_selector == version_range {
            continue;
        }
        let (selector_type, weight) = selector_info(entry);
        prioritizer.add_selector(meta, preferred_selector, selector_type, weight);
    }

    prioritizer.versions_by_priority()
}

/// Group-by-weight accumulator, including the quirk that weight `0`
/// acts as a sentinel a later non-zero `add` overwrites rather than
/// sums with.
#[derive(Default)]
pub(super) struct PreferredVersionsPrioritizer {
    pub(super) preferred_versions: BTreeMap<String, u32>,
}

impl PreferredVersionsPrioritizer {
    /// Weight every version one preferred selector names.
    pub(super) fn add_selector(
        &mut self,
        meta: &Package,
        selector: &str,
        selector_type: VersionSelectorType,
        weight: u32,
    ) {
        match selector_type {
            VersionSelectorType::Tag => {
                if let Some(version) = meta.dist_tag(selector) {
                    self.add(version.to_string(), weight);
                }
            }
            VersionSelectorType::Range => {
                for version in meta.versions.keys() {
                    if semver_satisfies_loose(version, selector) {
                        self.add(version.clone(), weight);
                    }
                }
            }
            VersionSelectorType::Version => {
                if meta.versions.contains_key(selector) {
                    self.add(selector.to_string(), weight);
                }
            }
        }
    }

    pub(super) fn add(&mut self, version: String, weight: u32) {
        let entry = self.preferred_versions.entry(version).or_insert(0);
        if *entry == 0 {
            // JS truthiness: `0` is falsy, so a later positive
            // weight replaces the seed. Once non-zero, further
            // adds sum normally.
            *entry = weight;
        } else {
            *entry += weight;
        }
    }

    pub(super) fn versions_by_priority(&self) -> Vec<Vec<String>> {
        let mut by_weight: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for (version, weight) in &self.preferred_versions {
            by_weight
                .entry(*weight)
                .or_default()
                .push(version.clone());
        }
        // Highest weight first. BTreeMap iterates lowest→highest, so
        // reverse explicitly.
        by_weight
            .into_iter()
            .rev()
            .map(|(_, group)| group)
            .collect()
    }
}
