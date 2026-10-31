use super::{
    LockedImporter, UpdatePlan, UpdateScope, create_matcher, ignored_resolved_names,
    matches_update_pattern, matching::selectors_match_ignored_direct,
    widen_drop_targets_by_matcher,
};
use crate::{UpdateSelection, UpdateVersionOptions};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use std::path::Path;

#[test]
fn negated_selectors_ignored_for_every_direct_dependency_are_a_noop() {
    let selectors =
        [super::super::selectors::ParsedSelector { pattern: "!other".to_string(), version: None }];
    let direct = vec![("foo".to_string(), DependencyGroup::Prod, "^1.0.0".to_string())];
    let mut config = Config::default();
    config.update_config.ignore_dependencies = Some(vec!["foo".to_string()]);

    assert!(selectors_match_ignored_direct(&selectors, &direct, &config));
}

#[test]
fn negated_selectors_without_direct_matches_are_not_ignored_noops() {
    let selectors =
        [super::super::selectors::ParsedSelector { pattern: "!foo".to_string(), version: None }];
    let direct = vec![("foo".to_string(), DependencyGroup::Prod, "^1.0.0".to_string())];
    let mut config = Config::default();
    config.update_config.ignore_dependencies = Some(vec!["foo".to_string()]);

    assert!(!selectors_match_ignored_direct(&selectors, &direct, &config));
}

#[test]
fn negated_configured_ignore_patterns_keep_their_matcher_semantics() {
    let selector_matcher = create_matcher(&["!bar".to_string()]);
    let ignore_matcher = create_matcher(&["!foo".to_string()]);

    assert!(matches_update_pattern(&selector_matcher, Some(&ignore_matcher), "foo"));
    assert!(!matches_update_pattern(&selector_matcher, Some(&ignore_matcher), "bar"));
    assert!(!matches_update_pattern(&selector_matcher, Some(&ignore_matcher), "baz"));
}

#[test]
fn ignored_aliases_keep_their_resolved_packages_out_of_lockfile_targets() {
    let direct = vec![("alias".to_string(), DependencyGroup::Prod, "npm:real@^1.0.0".to_string())];
    let lockfile = Lockfile::parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      alias:\n        specifier: npm:real@^1.0.0\n        version: real@1.0.0\nsnapshots:\n  real@1.0.0: {}\n",
        Path::new("pnpm-lock.yaml"),
    )
    .unwrap()
    .unwrap();
    let mut config = Config::default();
    config.update_config.ignore_dependencies = Some(vec!["alias".to_string()]);
    let scope = UpdateScope {
        selectors: &[],
        direct: &direct,
        overridden_direct: &[],
        lockfile: LockedImporter { document: Some(&lockfile), id: "." },
        config: &config,
        version: UpdateVersionOptions {
            latest: false,
            patches: false,
            range_spec_style: RangeSpecStyle::Major,
            save: false,
        },
        selection: UpdateSelection {
            packages: &[],
            depth: 0,
            workspace_packages: None,
            interactive: false,
            targets: None,
        },
        updates_all_groups: false,
    };
    let ignore_matcher = create_matcher(&["alias".to_string()]);
    let ignored_names = ignored_resolved_names(&scope, Some(&ignore_matcher));
    let mut plan = UpdatePlan::default();

    widen_drop_targets_by_matcher(
        Some(&lockfile),
        &mut plan,
        &create_matcher(&["!other".to_string()]),
        Some(&ignore_matcher),
        &ignored_names,
    );

    assert!(!plan.drop_targets.covers("real", None));
}
