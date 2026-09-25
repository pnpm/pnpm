use super::{PackageSelector, ResolvedOverride};
use node_semver::{Range, Version};

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

/// A target matches when its name equals `dep_name` and its range
/// intersects `dep_spec`.
pub(super) fn matches_target(target: &PackageSelector, dep_name: &str, dep_spec: &str) -> bool {
    target.name == dep_name && is_intersecting_range(target.bare_specifier.as_deref(), dep_spec)
}

/// Sort overrides so the "most specific" one — the one whose target
/// range is contained inside the others — sorts first.
/// The intuition is `b ⊃ a ⇒ a sorts before b`, so a narrower target
/// like `foo@1.2.3` wins over the broader `foo@^1`.
pub(super) fn sort_by_specificity(matching: &mut [&ResolvedOverride]) {
    matching.sort_by(|lhs, rhs| {
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
