use crate::pick_package_from_meta::{
    PickVersionByVersionRangeOptions, RegistryPackageSpec, RegistryPackageSpecType,
    apply_published_by_policy, pick_version_by_version_range,
};
use pnpm_registry::Package;
use pnpm_resolving_resolver_base::{
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
/// Surface that once per `(name, range, picked, preferred)`: reaching
/// the newer version everywhere is an override's job, not an update's.
///
/// The baseline for "held back" is the pick with only the non-pin
/// selectors applied — `range`/`tag` selectors such as the
/// `pnpm audit --fix` vulnerability penalties steer the baseline too,
/// so the warning never recommends a version those selectors avoid.
/// The baseline also honors the `published_by` maturity cutoff the
/// actual pick applied: a version blocked by `minimumReleaseAge` is
/// not an update the manifests held back, and recommending an
/// override for it would defeat the age gate.
///
/// The recommended override is scoped to the declared range being
/// resolved (`name@<range>`), so applying it can never violate any
/// consumer's range: only declarations of exactly this range match the
/// selector, and the recommended version satisfies it by construction.
pub(crate) fn warn_once_on_held_back_update(
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    selectors: Option<&VersionSelectors>,
    meta: &Package,
    picked_version: &str,
) {
    let Some(preferred) = held_back_preferred(opts, spec, selectors, meta, picked_version) else {
        return;
    };
    let key = format!("{}@{}:{picked_version}<{preferred}", spec.name, spec.fetch_spec);
    let mut warned = WARNED_HELD_BACK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if warned.contains(&key) {
        return;
    }
    if warned.len() >= MAX_WARNED_HELD_BACK {
        warned.shift_remove_index(0);
    }
    warned.insert(key);
    tracing::warn!(
        target: "pnpm_resolving_npm_resolver::preferred_overlay",
        pkg_name = spec.name,
        picked_version,
        preferred,
        r#""{}@{}" was updated to {picked_version}, not {preferred}, to match the version preferred by your manifests and already installed dependencies. To use {preferred}, add an override to pnpm-workspace.yaml: overrides: {{ "{}@{}": "{preferred}" }}"#,
        spec.name,
        spec.fetch_spec,
        spec.name,
        spec.fetch_spec,
    );
}

/// The version [`warn_once_on_held_back_update`] would recommend, or
/// `None` when the pick needs no warning because the baseline (see
/// there) already agrees with it.
fn held_back_preferred(
    opts: &ResolveOptions,
    spec: &RegistryPackageSpec,
    selectors: Option<&VersionSelectors>,
    meta: &Package,
    picked_version: &str,
) -> Option<String> {
    if !opts.update_requested || spec.spec_type != RegistryPackageSpecType::Range {
        return None;
    }
    let selectors = selectors?;
    let non_pin_selectors: VersionSelectors = selectors
        .iter()
        .filter(|(_, entry)| entry.selector_type() != VersionSelectorType::Version)
        .map(|(selector, entry)| (selector.clone(), entry.clone()))
        .collect();
    let view;
    // `needs_full_metadata` is not this caller's problem: the pick
    // already succeeded on this metadata, which for an abbreviated
    // packument means every version cleared the cutoff, so `meta` is
    // the filtered view.
    let baseline_meta: &Package = match opts.published_by {
        Some(cutoff) => {
            view = apply_published_by_policy(meta, cutoff, opts.published_by_exclude.as_ref());
            view.filtered.as_deref().unwrap_or(meta)
        }
        None => meta,
    };
    let preferred = pick_version_by_version_range(&PickVersionByVersionRangeOptions {
        meta: baseline_meta,
        version_range: &spec.fetch_spec,
        preferred_version_selectors: (!non_pin_selectors.is_empty()).then_some(&non_pin_selectors),
        published_by: opts.published_by,
    })?;
    (preferred != picked_version).then_some(preferred)
}

#[cfg(test)]
mod tests;
