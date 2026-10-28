use super::{DependencyGroup, PackageSelector, ResolvedOverride, VersionsOverrider};
use node_semver::{Range, Version};
use pnpm_config_parse_overrides::OverrideDependencyType;
use pnpm_matcher::WildcardMatcher;

impl VersionsOverrider {
    pub(super) fn choose_override<'b>(
        &'b self,
        group: Option<DependencyGroup>,
        applicable_parent_scoped: &[&'b ResolvedOverride],
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&'b ResolvedOverride> {
        Self::pick_most_specific(group, applicable_parent_scoped, dep_name, dep_spec)
            .or_else(|| self.pick_most_specific_generic(group, dep_name, dep_spec))
    }

    fn pick_most_specific<'b>(
        group: Option<DependencyGroup>,
        candidates: &[&'b ResolvedOverride],
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&'b ResolvedOverride> {
        let mut matching: Vec<&'b ResolvedOverride> = candidates
            .iter()
            .copied()
            .filter(|entry| matches_resolved_target(entry, group, dep_name, dep_spec))
            .collect();
        sort_by_specificity(&mut matching);
        matching.into_iter().next()
    }

    fn pick_most_specific_generic(
        &self,
        group: Option<DependencyGroup>,
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&ResolvedOverride> {
        let mut matching: Vec<&ResolvedOverride> = self.generic
            .iter()
            .filter(|entry| matches_resolved_target(entry, group, dep_name, dep_spec))
            .collect();
        sort_by_specificity(&mut matching);
        matching.into_iter().next()
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

/// A target matches when its name pattern matches `dep_name` and its range
/// intersects `dep_spec`.
pub(crate) fn matches_target(target: &PackageSelector, dep_name: &str, dep_spec: &str) -> bool {
    WildcardMatcher::new(&target.name).matches(dep_name)
        && is_intersecting_range(target.bare_specifier.as_deref(), dep_spec)
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

fn specificity_key(entry: &ResolvedOverride) -> (bool, bool, bool) {
    let parent_wildcard = entry.inner.parent_pkg
        .as_ref()
        .is_some_and(|parent| parent.name.contains('*'));
    let target_wildcard = entry.inner.target_pkg.name.contains('*');
    let untyped = entry.inner.target_pkg.dep_type.is_none();
    (parent_wildcard, target_wildcard, untyped)
}

fn compare_range_specificity(lhs: &ResolvedOverride, rhs: &ResolvedOverride) -> std::cmp::Ordering {
    let lhs_spec = lhs.inner.target_pkg.bare_specifier.as_deref().unwrap_or("");
    let rhs_spec = rhs.inner.target_pkg.bare_specifier.as_deref().unwrap_or("");
    // Rust's `sort_by` requires a total order, so the comparison
    // widens to a 3-way result: `lhs` is
    // strictly more specific when `rhs ⊇ lhs` but not vice versa,
    // strictly less specific in the mirror case, and equal when
    // both ranges cover each other (e.g. identical strings, or
    // mutually-intersecting unions). The `Equal` arm is what keeps
    // `sort_by`'s preconditions satisfied.
    let rhs_covers_lhs = is_intersecting_range(Some(rhs_spec), lhs_spec);
    let lhs_covers_rhs = is_intersecting_range(Some(lhs_spec), rhs_spec);
    match (rhs_covers_lhs, lhs_covers_rhs) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

/// Sort overrides so the most specific one sorts first:
/// 1. Exact parent name before wildcard parent name.
/// 2. Exact target name before wildcard target name.
/// 3. Dependency-type-qualified target (`:peer`, `:optional`, `:prod`) before untyped.
/// 4. Narrower target semver range (`b ⊃ a ⇒ a sorts before b`).
pub(super) fn sort_by_specificity(matching: &mut [&ResolvedOverride]) {
    matching.sort_by(|lhs, rhs| {
        specificity_key(lhs)
            .cmp(&specificity_key(rhs))
            .then_with(|| compare_range_specificity(lhs, rhs))
    });
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
