use super::{
    BTreeMap, BadPeerIssue, HashMap, HashSet, IssuesByProjects, MissingPeerIssue,
    PeerDependencyRules, PeerIssues, intersect_multiple_ranges, parse_wanted_dependency, satisfies,
};

pub(super) fn merge_missing_peers(
    missing: &BTreeMap<String, Vec<MissingPeerIssue>>,
) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut intersections = BTreeMap::new();

    for (peer_name, issues) in missing {
        if issues.iter().all(|issue| issue.optional) {
            continue;
        }
        if issues.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let ranges: Vec<&str> = issues.iter().map(|issue| issue.wanted_range.as_str()).collect();
        let unique: HashSet<&&str> = ranges.iter().collect();
        if unique.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let range_owned: Vec<String> =
            issues.iter().map(|issue| issue.wanted_range.clone()).collect();
        if let Some(intersection_str) = intersect_multiple_ranges(&range_owned) {
            intersections.insert(peer_name.clone(), intersection_str);
        } else {
            conflicts.push(peer_name.clone());
        }
    }

    MergeResult { conflicts, intersections }
}

pub(super) struct MergeResult {
    pub(super) conflicts: Vec<String>,
    pub(super) intersections: BTreeMap<String, String>,
}

#[must_use]
pub fn filter_peer_issues(
    mut issues: IssuesByProjects,
    rules: &PeerDependencyRules,
) -> IssuesByProjects {
    if rules.ignore_missing.is_none()
        && rules.allow_any.is_none()
        && rules.allowed_versions.is_none()
    {
        return issues;
    }

    let (allow_all_matcher, allow_by_parent) =
        parse_allowed_versions(&rules.allowed_versions.clone().unwrap_or_default());
    let ignore_missing_matcher =
        pnpm_matcher::create_matcher(&rules.ignore_missing.clone().unwrap_or_default());
    let allow_any_matcher =
        pnpm_matcher::create_matcher(&rules.allow_any.clone().unwrap_or_default());

    for project_issues in issues.values_mut() {
        filter_missing_issues(project_issues, &ignore_missing_matcher);

        project_issues.bad = project_issues
            .bad
            .iter()
            .filter(|(peer_name, _)| !allow_any_matcher.matches(peer_name))
            .filter_map(|(peer_name, peer_issues)| {
                let remaining: Vec<BadPeerIssue> = peer_issues
                    .iter()
                    .filter(|issue| {
                        !is_version_allowed(issue, peer_name, &allow_all_matcher, &allow_by_parent)
                    })
                    .cloned()
                    .collect();
                (!remaining.is_empty()).then(|| (peer_name.clone(), remaining))
            })
            .collect();

        let merged = merge_missing_peers(&project_issues.missing);
        project_issues.conflicts = merged.conflicts;
        project_issues.intersections = merged.intersections;
    }

    issues
}

fn filter_missing_issues(
    project_issues: &mut PeerIssues,
    ignore_missing_matcher: &pnpm_matcher::Matcher,
) {
    project_issues.missing = project_issues
        .missing
        .iter()
        .filter(|(peer_name, peer_issues)| {
            !ignore_missing_matcher.matches(peer_name)
                && !peer_issues.iter().all(|issue| issue.optional)
        })
        .map(|(peer_name, peer_issues)| (peer_name.clone(), peer_issues.clone()))
        .collect();
}

/// Whether an `allowedVersions` rule waives this mismatch, either
/// unconditionally for the peer or only under the parent that declares it.
fn is_version_allowed(
    issue: &BadPeerIssue,
    peer_name: &str,
    allow_all: &AllowAllMatcher,
    allow_by_parent: &AllowByParentMatcher,
) -> bool {
    if let Some(ranges) = allow_all.get(peer_name)
        && ranges.iter().any(|range| satisfies(&issue.found_version, range))
    {
        return true;
    }
    let Some(declaring_parent) = issue.parents.last() else {
        return false;
    };
    let Some(rules) = allow_by_parent.get(&declaring_parent.name) else {
        return false;
    };
    rules
        .iter()
        .filter(|rule| parent_range_matches(rule, &declaring_parent.version))
        .filter_map(|rule| rule.peer_rules.get(peer_name))
        .any(|ranges| ranges.iter().any(|range| satisfies(&issue.found_version, range)))
}

/// A rule with no parent range applies to every version of the parent.
fn parent_range_matches(rule: &ParentRule, parent_version: &str) -> bool {
    rule.parent_range.as_ref().is_none_or(|range| satisfies(parent_version, range))
}

type AllowAllMatcher = HashMap<String, Vec<String>>;

type AllowByParentMatcher = HashMap<String, Vec<ParentRule>>;

pub(super) struct ParentRule {
    parent_range: Option<String>,
    pub(super) peer_rules: HashMap<String, Vec<String>>,
}

pub(super) fn parse_allowed_versions(
    allowed: &BTreeMap<String, String>,
) -> (AllowAllMatcher, AllowByParentMatcher) {
    let mut match_all: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_parent: AllowByParentMatcher = HashMap::new();

    for (selector, spec) in allowed {
        if let Some((parent, target)) = selector.split_once('>') {
            add_parent_rule(&mut by_parent, parent, target, spec);
        } else {
            let parsed = parse_wanted_dependency(selector);
            let target_name = parsed.alias.unwrap_or_else(|| selector.clone());
            match_all.entry(target_name).or_default().extend(split_ranges(spec));
        }
    }

    (match_all, by_parent)
}

fn add_parent_rule(by_parent: &mut AllowByParentMatcher, parent: &str, target: &str, spec: &str) {
    let parsed_parent = parse_wanted_dependency(parent.trim());
    let parent_name = parsed_parent.alias.unwrap_or_else(|| parent.trim().to_string());
    let parent_range = parsed_parent.bare_specifier;

    let parsed_peer = parse_wanted_dependency(target.trim());
    let peer_name = parsed_peer.alias.unwrap_or_else(|| target.trim().to_string());

    let parent_entry = by_parent.entry(parent_name).or_default();
    if let Some(rule) =
        parent_entry.iter_mut().find(|rule_entry| rule_entry.parent_range == parent_range)
    {
        rule.peer_rules.entry(peer_name).or_default().extend(split_ranges(spec));
    } else {
        let mut peer_rules = HashMap::new();
        peer_rules.insert(peer_name, split_ranges(spec));
        parent_entry.push(ParentRule { parent_range, peer_rules });
    }
}

fn split_ranges(spec: &str) -> Vec<String> {
    spec.split("||").map(|seg| seg.trim().to_string()).collect()
}
