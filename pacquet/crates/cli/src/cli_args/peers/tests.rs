use std::collections::BTreeMap;

use super::*;

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

#[test]
fn test_satisfies_prerelease_tolerance() {
    assert!(satisfies("1.0.0-beta", "^1.0.0"));
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
    let r = vec!["^1.2.3".to_string(), ">=1.0.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&r).as_deref(), Some(">=1.2.3 <2.0.0"));
}

#[test]
fn test_intersect_multiple_ranges_conflict() {
    let r = vec!["^17.0.0".to_string(), "^18.0.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&r), None);
}

#[test]
fn test_intersect_multiple_ranges_exact() {
    let r = vec!["^16.0.0".to_string(), "16.1.0".to_string()];
    assert_eq!(intersect_multiple_ranges(&r).as_deref(), Some("16.1.0"));
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
    let result = merge_missing_peers(&HashMap::new());
    assert!(result.conflicts.is_empty());
    assert!(result.intersections.is_empty());
}

#[test]
fn test_merge_missing_peers_single() {
    let mut missing: HashMap<String, Vec<MissingPeerIssue>> = HashMap::new();
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
    let mut missing: HashMap<String, Vec<MissingPeerIssue>> = HashMap::new();
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
    let mut missing: HashMap<String, Vec<MissingPeerIssue>> = HashMap::new();
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
    let mut missing: HashMap<String, Vec<MissingPeerIssue>> = HashMap::new();
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
    let (match_all, by_parent) = parse_allowed_versions(&HashMap::new());
    assert!(match_all.is_empty());
    assert!(by_parent.is_empty());
}

#[test]
fn test_parse_allowed_versions_global() {
    let mut allowed = HashMap::new();
    allowed.insert("react".to_string(), "^18.0.0".to_string());
    let (match_all, by_parent) = parse_allowed_versions(&allowed);
    assert_eq!(match_all.len(), 1);
    assert_eq!(match_all["react"], vec!["^18.0.0"]);
    assert!(by_parent.is_empty());
}

#[test]
fn test_parse_allowed_versions_by_parent() {
    let mut allowed = HashMap::new();
    allowed.insert("@foo/bar>react".to_string(), "^18.0.0".to_string());
    let (match_all, by_parent) = parse_allowed_versions(&allowed);
    assert!(match_all.is_empty());
    assert_eq!(by_parent.len(), 1);
    assert_eq!(by_parent["@foo/bar"][0].peer_rules["react"], vec!["^18.0.0"]);
}

#[test]
fn test_parse_allowed_versions_mixed() {
    let mut allowed = HashMap::new();
    allowed.insert("react".to_string(), "^18.0.0".to_string());
    allowed.insert("@foo/bar>react".to_string(), "^17.0.0".to_string());
    let (match_all, by_parent) = parse_allowed_versions(&allowed);
    assert_eq!(match_all.len(), 1);
    assert_eq!(by_parent.len(), 1);
}

#[test]
fn test_filter_peer_issues_no_rules() {
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
    let mut issues: IssuesByProjects = HashMap::new();
    let mut peer = PeerIssues {
        bad: HashMap::new(),
        missing: HashMap::new(),
        conflicts: Vec::new(),
        intersections: HashMap::new(),
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
