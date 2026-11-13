use super::is_semver_subset;

#[test]
fn exact_and_tilde_ranges_are_subsets_of_enclosing_caret_range() {
    assert!(is_semver_subset("1.0.0", "^1.0.0"));
    assert!(!is_semver_subset("^1.0.0", "1.0.0"));
    assert!(is_semver_subset("~1.2.0", "^1.0.0"));
    assert!(!is_semver_subset("^1.0.0", "~1.2.0"));
}

#[test]
fn union_requires_every_branch_of_sub_to_be_covered() {
    assert!(is_semver_subset("^1.0.0", "^1.0.0 || ^2.0.0"));
    assert!(!is_semver_subset("^1.0.0 || ^2.0.0", "^1.0.0"));
    assert!(is_semver_subset("<1.0.0 || >=2.0.0 <2.1.0", "<3.0.0"));
    assert!(!is_semver_subset("<4.0.0", "<1.0.0 || >=2.0.0 <2.1.0"));
}

#[test]
fn partially_overlapping_intervals_are_incomparable() {
    assert!(!is_semver_subset(">=1.0.0 <2.0.0", ">=1.5.0 <3.0.0"));
    assert!(!is_semver_subset(">=1.5.0 <3.0.0", ">=1.0.0 <2.0.0"));
}

#[test]
fn prerelease_requires_matching_tuple_in_covering_branch() {
    assert!(!is_semver_subset("1.2.3-alpha.1", "^1.0.0"));
    assert!(is_semver_subset("1.2.3-alpha.1", "^1.2.3-alpha.0"));
    assert!(is_semver_subset("=1.0.0-alpha.1", "^1.0.0-alpha"));
    assert!(!is_semver_subset("1.2.3-alpha.2", ">=1.2.3-alpha.3 || ^1.0.0"));
    assert!(!is_semver_subset(">=1.0.0-alpha <1.0.0", "<1.0.0"));
    assert!(!is_semver_subset("^1.0.0-alpha", ">=0.0.0"));
    assert!(!is_semver_subset("~1.0.0-alpha", ">=0.0.0"));
    assert!(is_semver_subset("~1.0.0-alpha", "^1.0.0-alpha"));
    assert!(is_semver_subset(">=1.0.0 <1.2.3-0", "^1.0.0"));
    assert!(!is_semver_subset("<=1.2.3-0", "<2.0.0"));
}

#[test]
fn wildcard_and_empty_branches_cover_non_prereleases_only() {
    assert!(is_semver_subset("^1.0.0", "*"));
    assert!(!is_semver_subset("*", "^1.0.0"));
    assert!(is_semver_subset("*", ">=0.0.0"));
    assert!(is_semver_subset(">=0.0.0", "*"));
    assert!(is_semver_subset("", "*"));
    assert!(is_semver_subset("<1.0.0", "*"));
    assert!(!is_semver_subset("1.0.0-alpha.1", "*"));
}

#[test]
fn empty_intersection_branch_is_vacuously_covered() {
    assert!(is_semver_subset(">2.0.0 <1.0.0", "^1.0.0"));
    assert!(is_semver_subset(">2.0.0 <1.0.0 || ^1.2.0", "^1.0.0"));
    assert!(!is_semver_subset("^1.0.0", ">2.0.0 <1.0.0"));
}

#[test]
fn invalid_or_loose_garbage_tokens_are_rejected() {
    assert!(!is_semver_subset("latest", "*"));
    assert!(!is_semver_subset("^1.0.0", "latest"));
    assert!(!is_semver_subset("1.0.0 foo", "^1.0.0"));
    assert!(!is_semver_subset("1.0.0 - foo", "*"));
    assert!(!is_semver_subset("github:org/bar", "*"));
    assert!(!is_semver_subset("0.5.0", "<1.foo"));
    assert!(!is_semver_subset("1.0.0", "<2.garbage"));
    assert!(!is_semver_subset("1.1.0", "<1.2.foo"));
    assert!(!is_semver_subset("<1.foo", "*"));
    assert!(!is_semver_subset("<1.2.foo", "*"));
    assert!(!is_semver_subset("0.5.0", "<1."));
    assert!(!is_semver_subset("0.5.0", "<1.2.3.4"));
    assert!(is_semver_subset("0.5.0", "<1"));
    assert!(is_semver_subset("0.5.0", "<1.x"));
    assert!(is_semver_subset("1.1.0", "<1.2"));
    assert!(is_semver_subset("1.1.0", "<1.2.x"));
}
