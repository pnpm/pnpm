use crate::pick_package_from_meta::{
    PickVersionByVersionRangeOptions, RegistryPackageSpec, RegistryPackageSpecType,
    pick_version_by_version_range,
};
use pacquet_registry::Package;
use pacquet_resolving_resolver_base::{
    ResolveOptions, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
};
use std::sync::Mutex;

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

/// Bounds the held-back-update warn-once set; far above what one update
/// run realistically warns about. Oldest entries are evicted first via
/// ordered eviction with `shift_remove_index(0)`.
const MAX_WARNED_HELD_BACK: usize = 1024;
static WARNED_HELD_BACK: std::sync::LazyLock<Mutex<indexmap::IndexSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(indexmap::IndexSet::new()));

/// During a targeted update the picker still honors the preferred
/// versions a fresh install would apply (manifest pins and versions
/// propagated down the dependency chain), so the target can
/// legitimately settle below the highest version its range admits.
/// Surface that once per `(name, picked, preferred)`: reaching the
/// newer version everywhere is an override's job, not an update's.
///
/// The baseline for "held back" is the pick with only the non-pin
/// selectors applied — `range`/`tag` selectors such as the
/// `pnpm audit --fix` vulnerability penalties steer the baseline too,
/// so the warning never recommends a version those selectors avoid.
pub(crate) fn warn_once_on_held_back_update(
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    selectors: Option<&VersionSelectors>,
    meta: &Package,
    picked_version: &str,
) {
    if !opts.update_requested || spec.spec_type != RegistryPackageSpecType::Range {
        return;
    }
    let Some(selectors) = selectors else { return };
    let non_pin_selectors: VersionSelectors = selectors
        .iter()
        .filter(|(_, entry)| entry.selector_type() != VersionSelectorType::Version)
        .map(|(selector, entry)| (selector.clone(), entry.clone()))
        .collect();
    let Some(preferred) = pick_version_by_version_range(&PickVersionByVersionRangeOptions {
        meta,
        version_range: &spec.fetch_spec,
        preferred_version_selectors: (!non_pin_selectors.is_empty()).then_some(&non_pin_selectors),
        published_by: None,
    }) else {
        return;
    };
    if preferred == picked_version {
        return;
    }
    let key = format!("{}@{picked_version}<{preferred}", spec.name);
    let mut warned = WARNED_HELD_BACK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if warned.contains(&key) {
        return;
    }
    if warned.len() >= MAX_WARNED_HELD_BACK {
        warned.shift_remove_index(0);
    }
    warned.insert(key);
    tracing::warn!(
        target: "pacquet_resolving_npm_resolver::preferred_overlay",
        pkg_name = spec.name,
        picked_version,
        preferred,
        "\"{}\" was updated to {picked_version}, not {preferred}, to match the version preferred by your manifests and already installed dependencies. To use {preferred} everywhere, add an override: {{ \"pnpm\": {{ \"overrides\": {{ \"{}\": \"{preferred}\" }} }} }}",
        spec.name,
        spec.name,
    );
}
