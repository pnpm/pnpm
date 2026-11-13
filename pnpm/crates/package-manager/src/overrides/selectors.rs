use super::{DependencyGroup, PackageSelector, ResolvedOverride, VersionsOverrider};
use node_semver::{Range, Version};
use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config_parse_overrides::OverrideDependencyType;
use pnpm_matcher::WildcardMatcher;
use pnpm_semver_include_prerelease::is_semver_subset;
use serde_json::Value;
use std::borrow::Cow;

impl VersionsOverrider {
    pub(super) fn applicable_parent_scoped<'b>(
        &'b self,
        manifest: &Value,
    ) -> Vec<&'b ResolvedOverride> {
        let Some(manifest_name) = manifest.get("name").and_then(Value::as_str) else {
            return Vec::new();
        };
        let manifest_version = manifest.get("version").and_then(Value::as_str);

        self.parent_scoped
            .iter()
            .filter(|entry| {
                let Some(parent) = entry.inner.parent_pkg.as_ref() else { return false };
                let name_matches = entry.parent_name_matcher
                    .as_ref()
                    .is_some_and(|matcher| matcher.matches(manifest_name));
                let range_matches = match (parent.bare_specifier.as_deref(), manifest_version) {
                    (None, _) => true,
                    (Some(_), None) => false,
                    (Some(range), Some(version)) => semver_satisfies(version, range),
                };
                name_matches && range_matches
            })
            .collect()
    }

    pub(super) fn choose_override<'b>(
        &'b self,
        group: Option<DependencyGroup>,
        applicable_parent_scoped: &[&'b ResolvedOverride],
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&'b ResolvedOverride> {
        let effective_spec = resolve_catalog_spec(&self.catalogs, dep_name, dep_spec);
        Self::pick_most_specific(group, applicable_parent_scoped, dep_name, &effective_spec)
            .or_else(|| self.pick_most_specific_generic(group, dep_name, &effective_spec))
    }

    fn pick_most_specific<'b>(
        group: Option<DependencyGroup>,
        candidates: &[&'b ResolvedOverride],
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&'b ResolvedOverride> {
        let matching: Vec<&'b ResolvedOverride> = candidates
            .iter()
            .copied()
            .filter(|entry| matches_resolved_target(entry, group, dep_name, dep_spec))
            .collect();
        pick_most_specific_match(&matching)
    }

    fn pick_most_specific_generic(
        &self,
        group: Option<DependencyGroup>,
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&ResolvedOverride> {
        let matching: Vec<&ResolvedOverride> = self.generic
            .iter()
            .filter(|entry| matches_resolved_target(entry, group, dep_name, dep_spec))
            .collect();
        pick_most_specific_match(&matching)
    }
}

/// Parse a dependency edge's declared spec for the convergence
/// consult. Only plain semver ranges participate — `workspace:`,
/// `catalog:`, `npm:`, git/URL, and dist-tag specifiers have no
/// defined "satisfies" relation and yield `None`. An empty declared
/// spec counts as `*`.
pub(crate) fn parse_declared_range(spec: &str) -> Option<Range> {
    if spec.is_empty() {
        return Some(Range::any());
    }
    Range::parse(spec).ok()
}

/// A target matches when its dependency-type filter (if any) matches
/// `group`, its name pattern matches `dep_name`, and its range
/// intersects `dep_spec` (dereferenced through `catalogs` when `dep_spec`
/// uses the `catalog:` protocol).
pub(crate) fn matches_target(
    target: &PackageSelector,
    group: Option<DependencyGroup>,
    dep_name: &str,
    dep_spec: &str,
    catalogs: &Catalogs,
) -> bool {
    matches_dep_type(target.dep_type, group)
        && WildcardMatcher::new(&target.name).matches(dep_name)
        && is_intersecting_range(
            target.bare_specifier.as_deref(),
            &resolve_catalog_spec(catalogs, dep_name, dep_spec),
        )
}

/// The specifier a `catalog:` reference resolves to in `catalogs`, or
/// `dep_spec` when it is not a `catalog:` reference or has no valid entry.
fn resolve_catalog_spec<'a>(
    catalogs: &'a Catalogs,
    dep_name: &str,
    dep_spec: &'a str,
) -> Cow<'a, str> {
    if catalogs.is_empty() || !dep_spec.starts_with("catalog:") {
        return Cow::Borrowed(dep_spec);
    }
    match resolve_from_catalog(
        catalogs,
        &WantedDependency { alias: dep_name.to_string(), bare_specifier: dep_spec.to_string() },
        CatalogAnchor::AsWritten,
    ) {
        CatalogResolutionResult::Found(found) => Cow::Owned(found.resolution.specifier),
        CatalogResolutionResult::Misconfiguration(_) | CatalogResolutionResult::Unused => {
            Cow::Borrowed(dep_spec)
        }
    }
}

pub(super) fn matches_resolved_target(
    entry: &ResolvedOverride,
    group: Option<DependencyGroup>,
    dep_name: &str,
    dep_spec: &str,
) -> bool {
    matches_dep_type(entry.inner.target_pkg.dep_type, group)
        && entry.target_name_matcher.matches(dep_name)
        && is_intersecting_range(entry.inner.target_pkg.bare_specifier.as_deref(), dep_spec)
}

fn matches_dep_type(
    dep_type: Option<OverrideDependencyType>,
    group: Option<DependencyGroup>,
) -> bool {
    matches!(
        (dep_type, group),
        (None, _)
            | (Some(OverrideDependencyType::Prod), Some(DependencyGroup::Prod))
            | (Some(OverrideDependencyType::Optional), Some(DependencyGroup::Optional))
            | (Some(OverrideDependencyType::Peer), Some(DependencyGroup::Peer)),
    )
}

/// Field order is the sort priority: `false` sorts before `true`, so a
/// more specific selector sorts ahead of a broader one.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Specificity {
    has_wildcard_parent: bool,
    has_wildcard_target: bool,
    is_untyped_target: bool,
}

fn specificity_key(entry: &ResolvedOverride) -> Specificity {
    let has_wildcard_parent = entry.inner.parent_pkg
        .as_ref()
        .is_some_and(|parent| parent.name.contains('*'));
    let has_wildcard_target = entry.inner.target_pkg.name.contains('*');
    let is_untyped_target = entry.inner.target_pkg.dep_type.is_none();
    Specificity { has_wildcard_parent, has_wildcard_target, is_untyped_target }
}

/// Precomputes strict range dominance among peers in the same specificity
/// tier whose selector names can co-match, so per-dependency lookup never
/// reparses override ranges.
pub(super) fn populate_dominators(entries: &mut [ResolvedOverride]) {
    let keys: Vec<Specificity> = entries
        .iter()
        .map(specificity_key)
        .collect();
    let dominators: Vec<Vec<usize>> =
        (0..entries.len()).map(|idx| dominators_for_index(entries, &keys, idx)).collect();
    for (entry, dominated_by) in entries.iter_mut().zip(dominators) {
        entry.dominated_by = dominated_by;
    }
}

fn dominators_for_index(
    entries: &[ResolvedOverride],
    keys: &[Specificity],
    idx: usize,
) -> Vec<usize> {
    let candidate = &entries[idx];
    let key = &keys[idx];
    entries
        .iter()
        .enumerate()
        .filter(|&(other_idx, other)| {
            other_idx != idx
                && keys[other_idx] == *key
                && can_co_match(key, candidate, other)
                && is_strictly_narrower(other, candidate)
        })
        .map(|(_, other)| other.id)
        .collect()
}

fn can_co_match(key: &Specificity, lhs: &ResolvedOverride, rhs: &ResolvedOverride) -> bool {
    let targets_compatible = key.has_wildcard_target
        || lhs.inner.target_pkg.name == rhs.inner.target_pkg.name;
    let parents_compatible = key.has_wildcard_parent
        || lhs.inner.parent_pkg.as_ref().map(|pkg| &pkg.name)
            == rhs.inner.parent_pkg.as_ref().map(|pkg| &pkg.name);
    targets_compatible && parents_compatible
}

/// Select the first most-specific override among `matching`:
/// 1. Exact parent name before wildcard parent name.
/// 2. Exact target name before wildcard target name.
/// 3. Dependency-type-qualified target (`:peer`, `:optional`, `:prod`) before untyped.
/// 4. Narrower `(parent, target)` semver range (`b ⊃ a ⇒ a beats b`),
///    regardless of declaration order.
fn pick_most_specific_match<'b>(matching: &[&'b ResolvedOverride]) -> Option<&'b ResolvedOverride> {
    if matching.len() <= 1 {
        return matching.first().copied();
    }
    let best_key = matching
        .iter()
        .map(|entry| specificity_key(entry))
        .min()?;
    let top_tier: Vec<&'b ResolvedOverride> = matching
        .iter()
        .copied()
        .filter(|entry| specificity_key(entry) == best_key)
        .collect();
    if top_tier.len() <= 1 {
        return top_tier.first().copied();
    }
    top_tier
        .iter()
        .copied()
        .find(|candidate| {
            !candidate.dominated_by
                .iter()
                .any(|&dom_id| {
                    top_tier
                        .iter()
                        .any(|other| other.id == dom_id)
                })
        })
}

fn is_strictly_narrower(lhs: &ResolvedOverride, rhs: &ResolvedOverride) -> bool {
    selector_covers(rhs, lhs) && !selector_covers(lhs, rhs)
}

fn selector_covers(broad: &ResolvedOverride, narrow: &ResolvedOverride) -> bool {
    let broad_parent =
        broad.inner.parent_pkg.as_ref().and_then(|pkg| pkg.bare_specifier.as_deref());
    let narrow_parent =
        narrow.inner.parent_pkg.as_ref().and_then(|pkg| pkg.bare_specifier.as_deref());
    is_sub_range(broad_parent, narrow_parent)
        && is_sub_range(
            broad.inner.target_pkg.bare_specifier.as_deref(),
            narrow.inner.target_pkg.bare_specifier.as_deref(),
        )
}

fn is_sub_range(super_range: Option<&str>, sub_range: Option<&str>) -> bool {
    let Some(super_str) = super_range
        .map(str::trim)
        .filter(|range| !range.is_empty())
    else {
        return true;
    };
    let Some(sub_str) = sub_range
        .map(str::trim)
        .filter(|range| !range.is_empty())
    else {
        return false;
    };
    sub_str == super_str || is_semver_subset(sub_str, super_str)
}

/// An absent `range1` (or empty target spec) matches everything. An
/// exact-equal range pair matches without parsing. Otherwise both
/// sides must parse as semver and have a non-empty intersection.
fn is_intersecting_range(range1: Option<&str>, range2: &str) -> bool {
    let Some(range1_str) = range1 else { return true };
    if range1_str.is_empty() || range2 == range1_str {
        return true;
    }
    let Ok(parsed1) = range1_str.parse::<Range>() else { return false };
    let Ok(parsed2) = range2.parse::<Range>() else { return false };
    parsed1.allows_any(&parsed2)
}

/// True when `version` (parsed as a concrete semver) satisfies
/// `range`, the guard in the parent-scoped filter.
/// A non-parseable version OR range fails the match conservatively —
/// the parent constraint is treated as not applying.
pub(super) fn semver_satisfies(version: &str, range: &str) -> bool {
    let Ok(parsed_version) = version.parse::<Version>() else { return false };
    let Ok(parsed_range) = range.parse::<Range>() else { return false };
    parsed_range.satisfies(&parsed_version)
}
