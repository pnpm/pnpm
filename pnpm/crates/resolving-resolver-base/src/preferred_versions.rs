use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};

/// Allocation-friendly map type for [`PreferredVersions`].
///
/// `BTreeMap` (not `HashMap`) keeps iteration order stable across
/// runs, which matters because the deps-resolver consults these to
/// break version ties — a flapping order would let identical inputs
/// produce different lockfile picks.
pub type PreferredVersions = BTreeMap<String, VersionSelectors>;

/// Per-package set of selectors and their weights.
pub type VersionSelectors = BTreeMap<String, VersionSelectorEntry>;

/// Discriminator for how a selector should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionSelectorType {
    Version,
    Range,
    Tag,
}

/// One selector with a tie-break weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSelectorWithWeight {
    pub selector_type: VersionSelectorType,
    pub weight: u32,
}

/// A [`VersionSelectors`] map value: either a plain
/// [`VersionSelectorType`] or a [`VersionSelectorWithWeight`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSelectorEntry {
    Plain(VersionSelectorType),
    Weighted(VersionSelectorWithWeight),
}

impl VersionSelectorEntry {
    /// The selector's type, regardless of whether it carries a weight.
    #[must_use]
    pub fn selector_type(&self) -> VersionSelectorType {
        match self {
            VersionSelectorEntry::Plain(selector_type) => *selector_type,
            VersionSelectorEntry::Weighted(weighted) => weighted.selector_type,
        }
    }

    /// The selector's tie-break weight, defaulting to
    /// [`RESOLVED_VERSION_SELECTOR_WEIGHT`] for plain entries.
    #[must_use]
    pub fn weight(&self) -> u32 {
        match self {
            VersionSelectorEntry::Plain(_) => RESOLVED_VERSION_SELECTOR_WEIGHT,
            VersionSelectorEntry::Weighted(weighted) => weighted.weight,
        }
    }
}

/// One resolution level's preferred-version additions, layered over a
/// parent level: after a package's direct dependencies resolve, their
/// `(name, version)` pairs become plain `version` selectors for the
/// children's subtree resolutions, so a child's range prefers a version
/// one of its parent-level siblings pinned. Layering is O(1); lookups
/// walk the chain for one name.
#[derive(Debug)]
pub struct PreferredVersionsOverlay {
    layer: OverlayLayer,
    parent: Option<Arc<PreferredVersionsOverlay>>,
}

#[derive(Debug)]
enum OverlayLayer {
    Resolved(BTreeMap<String, Vec<String>>),
    DirectDeps(BTreeMap<String, Vec<String>>),
    /// The specifiers an importer's manifest declares. Unlike the
    /// resolved levels, its weight adds to theirs, the way a manifest entry
    /// in [`crate::VersionSelectionOptions::preferred_versions`] does.
    ManifestSpecifiers(PreferredVersions),
}

impl OverlayLayer {
    fn append_versions<'a>(&'a self, name: &str, versions: &mut Vec<&'a str>) {
        match self {
            OverlayLayer::Resolved(entries) | OverlayLayer::DirectDeps(entries) => {
                let Some(found) = entries.get(name) else { return };
                for version in found {
                    push_unique_version(versions, version.as_str());
                }
            }
            OverlayLayer::ManifestSpecifiers(entries) => {
                let Some(found) = entries.get(name) else { return };
                for (selector, entry) in found {
                    if entry.selector_type() == VersionSelectorType::Version {
                        push_unique_version(versions, selector.as_str());
                    }
                }
            }
        }
    }

    fn collect_weighted_selectors<'a>(
        &'a self,
        name: &str,
        selectors: &mut Vec<(&'a str, VersionSelectorType, u32)>,
        declared: &mut Vec<(&'a str, VersionSelectorType, u32)>,
    ) {
        let (entries, weight) = match self {
            OverlayLayer::Resolved(entries) => (entries, RESOLVED_VERSION_SELECTOR_WEIGHT),
            OverlayLayer::DirectDeps(entries) => (entries, DIRECT_DEP_SELECTOR_WEIGHT),
            OverlayLayer::ManifestSpecifiers(entries) => {
                let Some(found) = entries.get(name) else { return };
                declared.extend(
                    found
                        .iter()
                        .map(|(spec, entry)| {
                            (spec.as_str(), entry.selector_type(), entry.weight())
                        }),
                );
                return;
            }
        };
        let Some(found) = entries.get(name) else { return };
        for version in found {
            record_weighted_selector(selectors, version.as_str(), weight);
        }
    }
}

impl PreferredVersionsOverlay {
    /// Layer `entries` over `parent`. An empty layer collapses to the
    /// parent so chains only grow when a level resolved something.
    #[must_use]
    pub fn layer(
        parent: Option<Arc<PreferredVersionsOverlay>>,
        entries: BTreeMap<String, Vec<String>>,
    ) -> Option<Arc<PreferredVersionsOverlay>> {
        if entries.is_empty() {
            return parent;
        }
        Some(Arc::new(PreferredVersionsOverlay { layer: OverlayLayer::Resolved(entries), parent }))
    }

    /// Layer `entries` over `parent` as direct dependencies of an importer.
    #[must_use]
    pub fn layer_direct(
        parent: Option<Arc<PreferredVersionsOverlay>>,
        entries: BTreeMap<String, Vec<String>>,
    ) -> Option<Arc<PreferredVersionsOverlay>> {
        if entries.is_empty() {
            return parent;
        }
        Some(Arc::new(PreferredVersionsOverlay {
            layer: OverlayLayer::DirectDeps(entries),
            parent,
        }))
    }

    /// A root layer of the specifiers an importer's manifest declares.
    #[must_use]
    pub fn manifest_specifiers(
        entries: PreferredVersions,
    ) -> Option<Arc<PreferredVersionsOverlay>> {
        if entries.is_empty() {
            return None;
        }
        Some(Arc::new(PreferredVersionsOverlay {
            layer: OverlayLayer::ManifestSpecifiers(entries),
            parent: None,
        }))
    }

    /// Every version the chain prefers for `name`, nearest level
    /// first. Empty for names no level resolved.
    #[must_use]
    pub fn versions_for(&self, name: &str) -> Vec<&str> {
        let mut versions: Vec<&str> = Vec::new();
        let mut layer = Some(self);
        while let Some(current) = layer {
            layer = current.parent.as_deref();
            current.layer.append_versions(name, &mut versions);
        }
        versions
    }

    /// Every selector the chain prefers for `name`, nearest level first,
    /// tagged with its [`VersionSelectorType`] and weight
    /// ([`DIRECT_DEP_SELECTOR_WEIGHT`] for the importer direct-dependency level
    /// or [`RESOLVED_VERSION_SELECTOR_WEIGHT`] for descendant levels). Duplicate
    /// versions across levels retain the maximum weight. A manifest-specifiers
    /// layer adds its weight on top.
    #[must_use]
    pub fn weighted_selectors_for(&self, name: &str) -> Vec<(&str, VersionSelectorType, u32)> {
        let mut selectors: Vec<(&str, VersionSelectorType, u32)> = Vec::new();
        let mut declared: Vec<(&str, VersionSelectorType, u32)> = Vec::new();
        let mut layer = Some(self);
        while let Some(current) = layer {
            layer = current.parent.as_deref();
            current.layer.collect_weighted_selectors(name, &mut selectors, &mut declared);
        }
        for (spec, selector_type, weight) in declared {
            add_declared_selector(&mut selectors, spec, selector_type, weight);
        }
        selectors
    }

    /// Every selector the chain prefers for `name`, nearest level first,
    /// paired with its weight. See [`Self::weighted_selectors_for`].
    #[must_use]
    pub fn weighted_versions_for(&self, name: &str) -> Vec<(&str, u32)> {
        self.weighted_selectors_for(name)
            .into_iter()
            .map(|(spec, _, weight)| (spec, weight))
            .collect()
    }
}

fn push_unique_version<'a>(versions: &mut Vec<&'a str>, version: &'a str) {
    if !versions.contains(&version) {
        versions.push(version);
    }
}

fn record_weighted_selector<'a>(
    selectors: &mut Vec<(&'a str, VersionSelectorType, u32)>,
    version: &'a str,
    weight: u32,
) {
    if let Some(index) = selectors
        .iter()
        .position(|(v, t, _)| *v == version && *t == VersionSelectorType::Version)
    {
        selectors[index].2 = selectors[index].2.max(weight);
    } else {
        selectors.push((version, VersionSelectorType::Version, weight));
    }
}

fn add_declared_selector<'a>(
    selectors: &mut Vec<(&'a str, VersionSelectorType, u32)>,
    spec: &'a str,
    selector_type: VersionSelectorType,
    weight: u32,
) {
    match selectors
        .iter_mut()
        .find(|(s, t, _)| *s == spec && *t == selector_type)
    {
        Some((_, _, existing)) => *existing += weight,
        None => selectors.push((spec, selector_type, weight)),
    }
}

/// The weight of a version the wanted lockfile pins.
///
/// A selector's weight is the sum of the weights of its sources, one per
/// tier, so the sum orders the tiers and a version with several sources
/// outranks one with fewer. The picker takes the heaviest satisfying
/// version, highest first on a tie. Highest tier first:
///
/// 1. [`REQUESTED_VERSION_SELECTOR_WEIGHT`]: a version named on the
///    command line, or locked by the lockfile `pnpm import` reads.
/// 2. [`DIRECT_DEP_SELECTOR_WEIGHT`]: a specifier a project manifest
///    declares.
/// 3. [`RESOLVED_VERSION_SELECTOR_WEIGHT`]: a version this install already
///    resolved higher in the dependency chain, so a range dedupes onto it.
/// 4. [`EXISTING_VERSION_SELECTOR_WEIGHT`]: a version the wanted lockfile
///    pins. It breaks the tie against the versions nothing else prefers,
///    so an unchanged edge keeps its pin and a re-resolved edge lands
///    where a fresh install lands.
///
/// Each tier is a digit of a base-1000 number, so a tier's sources must
/// not add up past it. [`pinned_by_lockfile`] reads the lowest digit.
pub const EXISTING_VERSION_SELECTOR_WEIGHT: u32 = 1;

/// The weight of a version resolved higher in the dependency chain. See
/// [`EXISTING_VERSION_SELECTOR_WEIGHT`].
pub const RESOLVED_VERSION_SELECTOR_WEIGHT: u32 = 1_000;

/// The weight of a specifier a project manifest declares. See
/// [`EXISTING_VERSION_SELECTOR_WEIGHT`].
pub const DIRECT_DEP_SELECTOR_WEIGHT: u32 = 1_000_000;

/// The weight of a version named on the command line. See
/// [`EXISTING_VERSION_SELECTOR_WEIGHT`].
pub const REQUESTED_VERSION_SELECTOR_WEIGHT: u32 = 1_000_000_000;

/// Whether the wanted lockfile contributes to `weight`.
#[must_use]
pub const fn pinned_by_lockfile(weight: u32) -> bool {
    !weight.is_multiple_of(RESOLVED_VERSION_SELECTOR_WEIGHT)
}
