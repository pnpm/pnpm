use pacquet_resolving_resolver_base::{
    ResolveOptions, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
};

/// Drop selectors whose type is `version` (the propagated exact pins),
/// keeping `range` and `tag` selectors. Used for the user's update
/// target: the exact pins a sibling propagated must not hold the target
/// down, but `range`/`tag` selectors — e.g. the vulnerability-avoidance
/// penalties pnpm injects — must keep steering it. Mirrors pnpm's
/// [`stripVersionPins`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/npm-resolver/src/index.ts).
/// Returns `None` when nothing remains, so the caller treats it the same
/// as "no preferred versions".
pub(crate) fn strip_version_pins(selectors: &VersionSelectors) -> Option<VersionSelectors> {
    let kept: VersionSelectors = selectors
        .iter()
        .filter(|(_, entry)| entry.selector_type() != VersionSelectorType::Version)
        .map(|(selector, entry)| (selector.clone(), entry.clone()))
        .collect();
    (!kept.is_empty()).then_some(kept)
}

/// The picker's preferred selectors for `name` with the per-level
/// overlay folded in: each overlay version joins as a plain `version`
/// selector.
/// `None` when no level resolved this name; callers then borrow the
/// static map directly, so the owned merge allocates only on the rare
/// overlay hit.
pub(crate) fn overlay_merged_selectors(
    opts: &ResolveOptions,
    name: &str,
) -> Option<VersionSelectors> {
    let versions = opts.preferred_versions_overlay.as_ref()?.versions_for(name);
    if versions.is_empty() {
        return None;
    }
    let mut selectors = opts.preferred_versions.get(name).cloned().unwrap_or_default();
    for version in versions {
        selectors
            .entry(version.to_string())
            .or_insert(VersionSelectorEntry::Plain(VersionSelectorType::Version));
    }
    Some(selectors)
}
