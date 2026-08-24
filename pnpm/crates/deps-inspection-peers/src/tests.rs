use std::collections::BTreeMap;

use pnpm_config::PeerDependencyRules;

use super::{
    BadPeerIssue, IssuesByProjects, MissingPeerIssue, ParentPkg, PeerIssues, filter_peer_issues,
    format_range, intersect_multiple_ranges, merge_missing_peers, normalize_version_str,
    parse_allowed_versions, path_is_within, satisfies,
};

fn have_common_version(version_ranges: &[String]) -> bool {
    intersect_multiple_ranges(version_ranges).is_some()
}

#[test]
fn test_satisfies_exact_version() {
    assert!(satisfies("1.2.3", "1.2.3"));
}

#[test]
fn test_satisfies_caret_range() {
    assert!(satisfies("1.5.0", "^1.2.3"));
}

#[test]
fn test_satisfies_tilde_range() {
    assert!(satisfies("1.2.5", "~1.2.3"));
}

#[test]
fn test_satisfies_star() {
    assert!(satisfies("2.0.0", "*"));
}

#[test]
fn test_satisfies_fails() {
    assert!(!satisfies("2.0.0", "^1.0.0"));
}

/// pnpm matches peers with semver's `includePrerelease`, which admits a
/// prerelease anywhere inside the range's bounds but still orders it
/// below the release it precedes. Values checked against
/// `semver.satisfies(v, r, { includePrerelease: true, loose: true })`.
#[test]
fn test_satisfies_prerelease_matches_include_prerelease() {
    let cases = [
        // Inside the bounds: admitted, though no comparator carries a
        // prerelease of its own.
        ("1.5.0-beta", "^1.0.0", true),
        ("18.3.0-canary", "^18.0.0", true),
        ("1.0.0-rc.1", ">=0.9.0", true),
        ("2.0.0-beta.1", "^2.0.0-alpha", true),
        // Below the lower bound: a prerelease precedes its release.
        ("2.0.0-beta.1", "^2.0.0", false),
        ("1.0.0-rc.1", ">=1.0.0", false),
        ("1.0.0-beta", "^1.0.0", false),
        // At an upper bound npm derived rather than the user spelling
        // it out: `^2.0.0` reaches `<3.0.0-0`, so no prerelease of
        // 3.0.0 counts, while an explicit `<3.0.0` admits one.
        ("3.0.0-next.1", "^2.0.0", false),
        ("3.0.0-next.1", "<3.0.0", true),
        ("2.0.0-beta", "~1.9.0", false),
        ("1.9.5-beta", "~1.9.0", true),
        // Outside the range entirely.
        ("19.0.0-rc.1", "^16.8.4 || ^17.0.0 || ^18.0.0", false),
    ];
    for (version, range, expected) in cases {
        assert_eq!(satisfies(version, range), expected, "{version} against {range}");
    }
}

#[test]
fn test_satisfies_non_semver() {
    assert!(satisfies("custom-tag", "custom-tag"));
    assert!(!satisfies("0.0.0", "github:some/pkg"));
    assert!(!satisfies("1.0.0", "not-a-range"));
}

#[test]
fn test_normalize_version_str() {
    assert_eq!(normalize_version_str("1.x"), "1.0.0");
    assert_eq!(normalize_version_str("1.2.x"), "1.2.0");
    assert_eq!(normalize_version_str("1"), "1.0.0");
    assert_eq!(normalize_version_str("1.2.3-beta.0"), "1.2.3-beta.0");
}

#[test]
fn test_intersect_multiple_ranges_basic() {
    let version_ranges = vec!["^1.2.3".to_string(), ">=1.0.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&version_ranges).as_deref(), Some(">=1.2.3 <2.0.0"));
}

#[test]
fn test_intersect_multiple_ranges_conflict() {
    let version_ranges = vec!["^17.0.0".to_string(), "^18.0.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&version_ranges), None);
}

#[test]
fn test_intersect_multiple_ranges_exact() {
    let version_ranges = vec!["^16.0.0".to_string(), "16.1.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&version_ranges).as_deref(), Some("16.1.0"));
}

/// A range that leaves `minor` or `patch` unpinned reaches the next
/// level up, the way npm's own comparators do. Values checked against
/// `new semver.Range(r).range`, which is what pnpm's
/// `semver-range-intersect` agrees with.
#[test]
fn test_intersect_widens_partial_versions_like_npm() {
    let cases = [
        (vec!["~1", "1.5.0"], Some("1.5.0")),
        (vec!["~1.x", "1.5.0"], Some("1.5.0")),
        (vec!["1.x", "1.5.0"], Some("1.5.0")),
        (vec!["1", "1.5.0"], Some("1.5.0")),
        (vec!["^0", "0.5.0"], Some("0.5.0")),
        (vec!["^0.x", "0.5.0"], Some("0.5.0")),
        (vec!["1.2", "1.2.5"], Some("1.2.5")),
        (vec![">1.2", "1.2.5"], None),
        (vec!["<=1", "1.9.0"], Some("1.9.0")),
        // The pinned levels keep their tighter bounds.
        (vec!["~1.2", "1.3.0"], None),
        (vec!["^0.0", "0.1.0"], None),
        (vec!["~1", "2.0.0"], None),
    ];
    for (ranges, expected) in cases {
        let ranges: Vec<String> = ranges.into_iter().map(ToString::to_string).collect();
        let actual = intersect_multiple_ranges(&ranges);
        assert_eq!(actual.as_deref(), expected, "ranges: {ranges:?}");
    }
}

/// A `-0` the user wrote out is honored where it decides anything —
/// matching — and dropped where pnpm drops it: `semver-range-intersect`
/// renders `intersect("<2.0.0-0", ">=1.0.0")` as `>=1.0.0 <2.0.0`, so
/// rendering the suffix here would be the divergence, not hiding it.
#[test]
fn test_explicit_prerelease_upper_bound() {
    assert!(!satisfies("2.0.0-rc", "<2.0.0-0"));
    assert!(satisfies("2.0.0-rc", "<2.0.0"));
    assert!(satisfies("1.9.9", "<2.0.0-0"));

    let ranges = ["<2.0.0-0".to_string(), ">=1.0.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&ranges).as_deref(), Some(">=1.0.0 <2.0.0"));
}

#[test]
fn test_have_common_version_empty() {
    assert!(have_common_version(&[]));
}

#[test]
fn test_have_common_version_single() {
    assert!(have_common_version(&["^1.0.0".to_string()]));
}

#[test]
fn test_have_common_version_matching() {
    assert!(have_common_version(&["^1.2.3".to_string(), ">=1.0.0".to_string(),]));
}

#[test]
fn test_have_common_version_non_matching() {
    assert!(!have_common_version(&["^1.0.0".to_string(), "^2.0.0".to_string(),]));
}

#[test]
fn test_merge_missing_peers_empty() {
    let result = merge_missing_peers(&BTreeMap::new());
    assert!(result.conflicts.is_empty());
    assert!(result.intersections.is_empty());
}

#[test]
fn test_merge_missing_peers_single() {
    let mut missing: BTreeMap<String, Vec<MissingPeerIssue>> = BTreeMap::new();
    missing.insert(
        "react".to_string(),
        vec![MissingPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
        }],
    );
    let result = merge_missing_peers(&missing);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.intersections.len(), 1);
    assert_eq!(result.intersections["react"], "^18.0.0");
}

#[test]
fn test_merge_missing_peers_same_range() {
    let mut missing: BTreeMap<String, Vec<MissingPeerIssue>> = BTreeMap::new();
    missing.insert(
        "react".to_string(),
        vec![
            MissingPeerIssue {
                parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
                optional: false,
                wanted_range: "^18.0.0".to_string(),
            },
            MissingPeerIssue {
                parents: vec![ParentPkg { name: "bar".to_string(), version: "2.0.0".to_string() }],
                optional: false,
                wanted_range: "^18.0.0".to_string(),
            },
        ],
    );
    let result = merge_missing_peers(&missing);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.intersections.len(), 1);
}

#[test]
fn test_merge_missing_peers_conflicting() {
    let mut missing: BTreeMap<String, Vec<MissingPeerIssue>> = BTreeMap::new();
    missing.insert(
        "react".to_string(),
        vec![
            MissingPeerIssue {
                parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
                optional: false,
                wanted_range: "^17.0.0".to_string(),
            },
            MissingPeerIssue {
                parents: vec![ParentPkg { name: "bar".to_string(), version: "2.0.0".to_string() }],
                optional: false,
                wanted_range: "^18.0.0".to_string(),
            },
        ],
    );
    let result = merge_missing_peers(&missing);
    assert_eq!(result.conflicts.len(), 1);
    assert!(result.conflicts.contains(&"react".to_string()));
    assert!(result.intersections.is_empty());
}

#[test]
fn test_merge_missing_peers_all_optional_skipped() {
    let mut missing: BTreeMap<String, Vec<MissingPeerIssue>> = BTreeMap::new();
    missing.insert(
        "react".to_string(),
        vec![
            MissingPeerIssue {
                parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
                optional: true,
                wanted_range: "^18.0.0".to_string(),
            },
            MissingPeerIssue {
                parents: vec![ParentPkg { name: "bar".to_string(), version: "2.0.0".to_string() }],
                optional: true,
                wanted_range: "^18.0.0".to_string(),
            },
        ],
    );
    let result = merge_missing_peers(&missing);
    assert!(result.conflicts.is_empty());
    assert!(result.intersections.is_empty());
}

#[test]
fn test_parse_allowed_versions_empty() {
    let (match_all, by_parent) = parse_allowed_versions(&BTreeMap::new());
    assert!(match_all.is_empty());
    assert!(by_parent.is_empty());
}

#[test]
fn test_parse_allowed_versions_global() {
    let mut allowed = BTreeMap::new();
    allowed.insert("react".to_string(), "^18.0.0".to_string());
    let (match_all, by_parent) = parse_allowed_versions(&allowed);
    assert_eq!(match_all.len(), 1);
    assert_eq!(match_all["react"], vec!["^18.0.0"]);
    assert!(by_parent.is_empty());
}

#[test]
fn test_parse_allowed_versions_by_parent() {
    let mut allowed = BTreeMap::new();
    allowed.insert("@foo/bar>react".to_string(), "^18.0.0".to_string());
    let (match_all, by_parent) = parse_allowed_versions(&allowed);
    assert!(match_all.is_empty());
    assert_eq!(by_parent.len(), 1);
    assert_eq!(by_parent["@foo/bar"][0].peer_rules["react"], vec!["^18.0.0"]);
}

#[test]
fn test_parse_allowed_versions_mixed() {
    let mut allowed = BTreeMap::new();
    allowed.insert("react".to_string(), "^18.0.0".to_string());
    allowed.insert("@foo/bar>react".to_string(), "^17.0.0".to_string());
    let (match_all, by_parent) = parse_allowed_versions(&allowed);
    assert_eq!(match_all.len(), 1);
    assert_eq!(by_parent.len(), 1);
}

#[test]
fn test_filter_peer_issues_no_rules() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.bad.insert(
        "react".to_string(),
        vec![BadPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
            found_version: "17.0.0".to_string(),
            resolved_from: Vec::new(),
        }],
    );
    issues.insert("project".to_string(), peer);
    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules { ignore_missing: None, allow_any: None, allowed_versions: None },
    );
    assert_eq!(filtered["project"].bad.len(), 1);
    assert!(!filtered["project"].bad["react"].is_empty());
}

#[test]
fn test_filter_peer_issues_allow_any() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.bad.insert(
        "react".to_string(),
        vec![BadPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
            found_version: "17.0.0".to_string(),
            resolved_from: Vec::new(),
        }],
    );
    issues.insert("project".to_string(), peer);
    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules {
            ignore_missing: None,
            allow_any: Some(vec!["react".to_string()]),
            allowed_versions: None,
        },
    );
    assert!(filtered["project"].bad.is_empty());
}

#[test]
fn test_filter_peer_issues_allowed_versions() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.bad.insert(
        "react".to_string(),
        vec![BadPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
            found_version: "17.0.0".to_string(),
            resolved_from: Vec::new(),
        }],
    );
    issues.insert("project".to_string(), peer);
    let mut allowed = BTreeMap::new();
    allowed.insert("react".to_string(), "^17.0.0".to_string());
    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules {
            ignore_missing: None,
            allow_any: None,
            allowed_versions: Some(allowed),
        },
    );
    assert!(filtered["project"].bad.is_empty());
}

#[test]
fn test_filter_peer_issues_allowed_versions_not_matching() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.bad.insert(
        "react".to_string(),
        vec![BadPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
            found_version: "16.0.0".to_string(),
            resolved_from: Vec::new(),
        }],
    );
    issues.insert("project".to_string(), peer);
    let mut allowed = BTreeMap::new();
    allowed.insert("react".to_string(), "^17.0.0".to_string());
    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules {
            ignore_missing: None,
            allow_any: None,
            allowed_versions: Some(allowed),
        },
    );
    assert_eq!(filtered["project"].bad.len(), 1);
}

#[test]
fn test_filter_peer_issues_ignore_missing() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.missing.insert(
        "react".to_string(),
        vec![MissingPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
        }],
    );
    issues.insert("project".to_string(), peer);
    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules {
            ignore_missing: Some(vec!["react".to_string()]),
            allow_any: None,
            allowed_versions: None,
        },
    );
    assert!(filtered["project"].missing.is_empty());
}

#[test]
fn test_filter_peer_issues_ignore_missing_pattern() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.missing.insert(
        "@scope/pkg".to_string(),
        vec![MissingPeerIssue {
            parents: vec![ParentPkg { name: "foo".to_string(), version: "1.0.0".to_string() }],
            optional: false,
            wanted_range: "^1.0.0".to_string(),
        }],
    );
    issues.insert("project".to_string(), peer);
    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules {
            ignore_missing: Some(vec!["@scope/*".to_string()]),
            allow_any: None,
            allowed_versions: None,
        },
    );
    assert!(filtered["project"].missing.is_empty());
}

#[test]
fn test_filter_peer_issues_allowed_versions_parent_scoped() {
    let mut issues: IssuesByProjects = BTreeMap::new();
    let mut peer = PeerIssues {
        bad: BTreeMap::new(),
        missing: BTreeMap::new(),
        conflicts: Vec::new(),
        intersections: BTreeMap::new(),
    };
    peer.bad.insert(
        "react".to_string(),
        vec![BadPeerIssue {
            parents: vec![ParentPkg { name: "@foo/bar".to_string(), version: "1.2.3".to_string() }],
            optional: false,
            wanted_range: "^18.0.0".to_string(),
            found_version: "17.0.0".to_string(),
            resolved_from: Vec::new(),
        }],
    );
    issues.insert("project".to_string(), peer);

    let mut allowed = BTreeMap::new();
    allowed.insert("@foo/bar@^1.0.0>react".to_string(), "^17.0.0".to_string());

    let filtered = filter_peer_issues(
        issues,
        &PeerDependencyRules {
            ignore_missing: None,
            allow_any: None,
            allowed_versions: Some(allowed),
        },
    );
    assert!(filtered["project"].bad.is_empty());
}

#[test]
fn test_format_range_simple() {
    assert_eq!(format_range("^1.2.3"), "^1.2.3");
}

#[test]
fn test_format_range_with_space() {
    assert_eq!(format_range(">=1.0.0 <2.0.0"), r#"">=1.0.0 <2.0.0""#);
}

#[test]
fn test_format_range_wildcard() {
    assert_eq!(format_range("*"), r#""*""#);
}

#[test]
fn test_path_is_within() {
    let temp_dir = tempfile::tempdir().unwrap();
    let base = temp_dir.path();
    let sub = base.join("foo");
    std::fs::create_dir(&sub).unwrap();

    assert!(path_is_within(&sub, base));
    assert!(path_is_within(base, base));

    let outside = base.join("../bar");
    assert!(!path_is_within(&outside, base));

    let absolute_outside = std::path::Path::new("/etc");
    assert!(!path_is_within(absolute_outside, base));
}
