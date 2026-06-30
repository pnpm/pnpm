use pacquet_resolving_resolver_base::{
    ResolveOptions, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
};

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
