use super::{super::latest_subtree::unmatched_direct_names, UpdateScope};
use pnpm_matcher::{Matcher, create_matcher};
use std::collections::HashSet;

pub(crate) fn ignored_resolved_names(
    scope: &UpdateScope<'_>,
    ignore_matcher: Option<&Matcher>,
) -> HashSet<String> {
    let Some(ignore_matcher) = ignore_matcher else { return HashSet::new() };
    let matched = scope.direct
        .iter()
        .map(|(name, _, _)| name.as_str())
        .filter(|name| !ignore_matcher.matches(name))
        .collect::<HashSet<_>>();
    scope.lockfile.document
        .map(|lockfile| unmatched_direct_names(scope, lockfile, &matched))
        .unwrap_or_default()
}

pub(crate) fn matches_update_pattern(
    matcher: &Matcher,
    ignore_matcher: Option<&Matcher>,
    name: &str,
) -> bool {
    matcher.matches(name) && !ignore_matcher.is_some_and(|matcher| matcher.matches(name))
}

pub(super) fn update_matcher_patterns(scope: &UpdateScope<'_>) -> Vec<String> {
    scope.selectors
        .iter()
        .map(|selector| selector.pattern.clone())
        .collect()
}

pub(super) fn update_ignore_matcher(scope: &UpdateScope<'_>) -> Option<Matcher> {
    configured_ignore_matcher(scope.selectors, scope.config)
}

pub(in crate::update) fn selectors_match_ignored_direct(
    selectors: &[super::super::selectors::ParsedSelector],
    direct: &[(String, pnpm_package_manifest::DependencyGroup, String)],
    config: &pnpm_config::Config,
) -> bool {
    let matcher = create_matcher(
        &selectors
            .iter()
            .map(|selector| selector.pattern.clone())
            .collect::<Vec<_>>(),
    );
    let ignore_matcher = configured_ignore_matcher(selectors, config);
    direct
        .iter()
        .any(|(name, _, _)| matcher.matches(name))
        && !direct
            .iter()
            .any(|(name, _, _)| matches_update_pattern(&matcher, ignore_matcher.as_ref(), name))
}

fn configured_ignore_matcher(
    selectors: &[super::super::selectors::ParsedSelector],
    config: &pnpm_config::Config,
) -> Option<Matcher> {
    selectors
        .iter()
        .all(|selector| selector.pattern.starts_with('!'))
        .then(|| config.update_config.ignore_dependencies.as_deref().unwrap_or_default())
        .filter(|patterns| !patterns.is_empty())
        .map(create_matcher)
}
