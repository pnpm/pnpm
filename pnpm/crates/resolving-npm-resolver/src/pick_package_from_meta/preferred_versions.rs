use super::{
    BTreeMap, EXISTING_VERSION_SELECTOR_WEIGHT, Package, VersionSelectorEntry, VersionSelectorType,
    VersionSelectors, semver_satisfies_loose,
};

pub(crate) fn dominant_lockfile_version(
    version_range: &str,
    preferred_version_selectors: Option<&VersionSelectors>,
) -> Option<String> {
    let selectors = preferred_version_selectors?;
    let lockfile_version = sole_lockfile_selector(version_range, selectors)?;
    let weights = selector_weights(version_range, selectors, &lockfile_version)?;
    (weights.guaranteed > weights.maximum_other).then_some(lockfile_version)
}

/// The one high-weight exact selector that satisfies `version_range`, or
/// `None` when there is no such selector, several of them, or a
/// zero-weighted one (the prioritizer's replaceable seed sentinel).
pub(super) fn sole_lockfile_selector(
    version_range: &str,
    selectors: &VersionSelectors,
) -> Option<String> {
    let mut lockfile_version: Option<String> = None;
    for (selector, entry) in selectors.iter().filter(|(selector, _)| *selector != version_range) {
        let (selector_type, weight) = selector_info(entry);
        if weight == 0 {
            return None;
        }
        let pins_a_version = selector_type == VersionSelectorType::Version
            && weight >= EXISTING_VERSION_SELECTOR_WEIGHT
            && semver_satisfies_loose(selector, version_range);
        if !pins_a_version {
            continue;
        }
        if lockfile_version.is_some() {
            return None;
        }
        lockfile_version = Some(selector.clone());
    }
    lockfile_version
}

/// How much weight is guaranteed to land on the lockfile version, against
/// the most any other version could accumulate.
pub(super) struct SelectorWeights {
    pub(super) guaranteed: u64,
    pub(super) maximum_other: u64,
}

/// Every range and movable tag may apply to an unseen version, while an
/// out-of-range high-weight exact selector is discarded by max/min
/// satisfying.
pub(super) fn selector_weights(
    version_range: &str,
    selectors: &VersionSelectors,
    lockfile_version: &str,
) -> Option<SelectorWeights> {
    let mut weights = SelectorWeights { guaranteed: 0, maximum_other: 0 };
    for (selector, entry) in selectors {
        if selector == version_range {
            continue;
        }
        let (selector_type, weight) = selector_info(entry);
        let weight = u64::from(weight);
        match selector_type {
            VersionSelectorType::Version => {
                add_version_weight(
                    &mut weights,
                    version_range,
                    selector,
                    weight,
                    lockfile_version,
                )?;
            }
            VersionSelectorType::Range => {
                if semver_satisfies_loose(lockfile_version, selector) {
                    weights.guaranteed = weights.guaranteed.checked_add(weight)?;
                }
                weights.maximum_other = weights.maximum_other.checked_add(weight)?;
            }
            VersionSelectorType::Tag => {
                weights.maximum_other = weights.maximum_other.checked_add(weight)?;
            }
        }
    }
    Some(weights)
}

pub(super) fn add_version_weight(
    weights: &mut SelectorWeights,
    version_range: &str,
    selector: &str,
    weight: u64,
    lockfile_version: &str,
) -> Option<()> {
    if selector == lockfile_version {
        weights.guaranteed = weights.guaranteed.checked_add(weight)?;
        return Some(());
    }
    if weight < u64::from(EXISTING_VERSION_SELECTOR_WEIGHT)
        && semver_satisfies_loose(selector, version_range)
    {
        weights.maximum_other = weights.maximum_other.checked_add(weight)?;
    }
    Some(())
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
            by_weight.entry(*weight).or_default().push(version.clone());
        }
        // Highest weight first. BTreeMap iterates lowest→highest, so
        // reverse explicitly.
        by_weight.into_iter().rev().map(|(_, group)| group).collect()
    }
}
