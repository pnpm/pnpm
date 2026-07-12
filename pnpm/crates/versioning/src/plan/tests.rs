use std::{collections::HashSet, path::PathBuf};

use indexmap::IndexMap;
use pretty_assertions::assert_eq;

use super::{
    AssembleReleasePlanOptions, DependencyField, DependencyUpdate, ManifestDependency,
    PlannedRelease, ReleasePlan, WorkspaceProject, assemble_release_plan,
    materialize_workspace_range,
};
use crate::{
    intents::{ChangeIntent, IntentBumpType},
    ledger::Ledger,
    settings::{ReleaseBumpType, VersioningSettings},
};

fn make_project(name: &str, version: &str, deps: &[(&str, &str)]) -> WorkspaceProject {
    WorkspaceProject {
        root_dir: PathBuf::from(format!("/ws/{name}")),
        name: Some(name.to_string()),
        version: Some(version.to_string()),
        prod_dependencies: deps
            .iter()
            .map(|(alias, spec)| ManifestDependency {
                field: DependencyField::Dependencies,
                alias: (*alias).to_string(),
                spec: (*spec).to_string(),
            })
            .collect(),
    }
}

fn bump(value: &str) -> IntentBumpType {
    match value {
        "none" => IntentBumpType::None,
        "patch" => IntentBumpType::Patch,
        "minor" => IntentBumpType::Minor,
        "major" => IntentBumpType::Major,
        _ => panic!("unknown bump type in test fixture: {value}"),
    }
}

fn make_intent(id: &str, releases: &[(&str, &str)]) -> ChangeIntent {
    ChangeIntent {
        id: id.to_string(),
        file_path: PathBuf::from(format!("/ws/.changeset/{id}.md")),
        releases: releases
            .iter()
            .map(|(name, bump_type)| ((*name).to_string(), bump(bump_type)))
            .collect::<IndexMap<String, IntentBumpType>>(),
        summary: format!("summary of {id}"),
    }
}

fn ledger(entries: &[(&str, &[&str])]) -> Ledger {
    entries
        .iter()
        .map(|(key, ids)| ((*key).to_string(), ids.iter().map(|id| (*id).to_string()).collect()))
        .collect()
}

fn assemble(
    projects: &[WorkspaceProject],
    intents: &[ChangeIntent],
    consumed: &Ledger,
    versioning: Option<&VersioningSettings>,
) -> ReleasePlan {
    assemble_release_plan(
        projects,
        intents,
        consumed,
        versioning,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles")
}

fn release<'a>(plan: &'a ReleasePlan, name: &str) -> &'a PlannedRelease {
    plan.releases.iter().find(|release| release.name == name).expect("release is planned")
}

fn release_names(plan: &ReleasePlan) -> Vec<&str> {
    plan.releases.iter().map(|release| release.name.as_str()).collect()
}

#[test]
fn direct_bumps_highest_pending_bump_type_wins_per_package() {
    let projects = [make_project("a", "1.0.0", &[]), make_project("b", "2.3.4", &[])];
    let intents = [
        make_intent("one", &[("a", "patch"), ("b", "minor")]),
        make_intent("two", &[("a", "minor")]),
    ];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(plan.releases.len(), 2);
    assert_eq!(release(&plan, "a").new_version, "1.1.0");
    assert_eq!(release(&plan, "a").bump_type, ReleaseBumpType::Minor);
    assert_eq!(release(&plan, "b").new_version, "2.4.0");
    assert_eq!(release(&plan, "b").bump_type, ReleaseBumpType::Minor);
}

#[test]
fn intents_already_recorded_in_the_ledger_are_not_consumed_again() {
    let projects = [make_project("a", "1.0.1", &[])];
    let intents = [make_intent("one", &[("a", "patch")])];
    let consumed = ledger(&[("a@1.0.1", &["one"])]);
    let plan = assemble(&projects, &intents, &consumed, None);
    assert_eq!(plan.releases.len(), 0);
}

#[test]
fn a_none_bump_type_releases_nothing() {
    let projects = [make_project("a", "1.0.0", &[])];
    let intents = [make_intent("one", &[("a", "none")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(plan.releases.len(), 0);
}

#[test]
fn dependent_propagation_follows_the_materialized_workspace_range() {
    let projects = [
        make_project("lib", "1.2.0", &[]),
        make_project("cli", "3.0.0", &[("lib", "workspace:^")]),
    ];
    let intents = [make_intent("one", &[("lib", "major")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    let cli = release(&plan, "cli");
    assert_eq!(cli.new_version, "3.0.1");
    assert_eq!(cli.bump_type, ReleaseBumpType::Patch);
    assert_eq!(
        cli.dependency_updates,
        vec![DependencyUpdate { name: "lib".to_string(), new_version: "2.0.0".to_string() }]
    );
}

#[test]
fn a_minor_bump_does_not_propagate_through_workspace_caret_on_a_1x_dependency() {
    let projects = [
        make_project("lib", "1.2.0", &[]),
        make_project("cli", "3.0.0", &[("lib", "workspace:^")]),
    ];
    let intents = [make_intent("one", &[("lib", "minor")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(release_names(&plan), ["lib"]);
}

#[test]
fn a_minor_bump_propagates_through_workspace_caret_on_a_0x_dependency() {
    let projects = [
        make_project("lib", "0.2.0", &[]),
        make_project("cli", "3.0.0", &[("lib", "workspace:^")]),
    ];
    let intents = [make_intent("one", &[("lib", "minor")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(release_names(&plan), ["cli", "lib"]);
}

#[test]
fn propagation_cascades_through_chains_of_dependents() {
    let projects = [
        make_project("core", "1.0.0", &[]),
        make_project("mid", "1.0.0", &[("core", "workspace:*")]),
        make_project("top", "1.0.0", &[("mid", "workspace:*")]),
    ];
    let intents = [make_intent("one", &[("core", "patch")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(release_names(&plan), ["core", "mid", "top"]);
}

#[test]
fn fixed_groups_release_together_at_one_shared_version() {
    let projects = [make_project("a", "1.2.0", &[]), make_project("b", "1.0.5", &[])];
    let intents = [make_intent("one", &[("a", "minor")])];
    let versioning = VersioningSettings {
        fixed: vec![vec!["a".to_string(), "b".to_string()]],
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "a").new_version, "1.3.0");
    assert_eq!(release(&plan, "b").new_version, "1.3.0");
}

#[test]
fn ignored_packages_neither_release_nor_propagate() {
    let projects = [
        make_project("lib", "1.0.0", &[]),
        make_project("frozen", "1.0.0", &[("lib", "workspace:*")]),
    ];
    let intents = [make_intent("one", &[("lib", "major")])];
    let versioning =
        VersioningSettings { ignore: vec!["frozen".to_string()], ..VersioningSettings::default() };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release_names(&plan), ["lib"]);
}

#[test]
fn an_internal_dependency_without_the_workspace_protocol_fails_the_plan() {
    let projects =
        [make_project("lib", "1.0.0", &[]), make_project("cli", "1.0.0", &[("lib", "^1.0.0")])];
    let err = assemble_release_plan(
        &projects,
        &[],
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("workspace: protocol"), "unexpected error: {err}");
}

#[test]
fn an_npm_alias_colliding_with_a_workspace_package_name_is_not_an_internal_dependency() {
    let projects = [
        make_project("lib", "1.0.0", &[]),
        make_project("cli", "1.0.0", &[("lib", "npm:some-fork@^1.0.0")]),
    ];
    let intents = [make_intent("one", &[("lib", "major")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(release_names(&plan), ["lib"]);
}

#[test]
fn an_intent_demanding_a_release_of_an_unreleasable_package_fails_the_plan() {
    let projects = [make_project("lib", "1.0.0", &[]), make_project("frozen", "1.0.0", &[])];
    let intents = [make_intent("one", &[("frozen", "patch"), ("lib", "patch")])];
    let versioning =
        VersioningSettings { ignore: vec!["frozen".to_string()], ..VersioningSettings::default() };
    let err = assemble_release_plan(
        &projects,
        &intents,
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("cannot release"), "unexpected error: {err}");
}

#[test]
fn a_none_decline_for_an_unreleasable_package_is_accepted() {
    let projects = [make_project("lib", "1.0.0", &[]), make_project("frozen", "1.0.0", &[])];
    let intents = [make_intent("one", &[("frozen", "none"), ("lib", "patch")])];
    let versioning =
        VersioningSettings { ignore: vec!["frozen".to_string()], ..VersioningSettings::default() };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release_names(&plan), ["lib"]);
}

#[test]
fn an_intent_naming_an_unknown_package_fails_the_plan() {
    let projects = [make_project("lib", "1.0.0", &[])];
    let intents = [make_intent("one", &[("ghost", "patch")])];
    let err = assemble_release_plan(
        &projects,
        &intents,
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("not a package in this workspace"), "unexpected error: {err}");
}

#[test]
fn max_bump_rejects_a_plan_whose_effective_bump_exceeds_the_cap() {
    let projects = [make_project("lib", "1.0.0", &[])];
    let intents = [make_intent("one", &[("lib", "minor")])];
    let versioning = VersioningSettings {
        max_bump: Some(ReleaseBumpType::Patch),
        ..VersioningSettings::default()
    };
    let err = assemble_release_plan(
        &projects,
        &intents,
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("maxBump"), "unexpected error: {err}");
}

#[test]
fn max_bump_measures_the_real_version_distance_including_fixed_group_jumps() {
    let projects = [make_project("a", "1.0.5", &[]), make_project("b", "2.0.0", &[])];
    let intents = [make_intent("one", &[("a", "minor")])];
    let versioning = VersioningSettings {
        fixed: vec![vec!["a".to_string(), "b".to_string()]],
        max_bump: Some(ReleaseBumpType::Minor),
        ..VersioningSettings::default()
    };
    let err = assemble_release_plan(
        &projects,
        &intents,
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("maxBump"), "unexpected error: {err}");
}

fn on_line(pkg_name: &str, tag: &str) -> VersioningSettings {
    VersioningSettings {
        prereleases: IndexMap::from([(pkg_name.to_string(), tag.to_string())]),
        ..VersioningSettings::default()
    }
}

#[test]
fn a_package_on_a_prerelease_line_emits_tagged_versions_with_an_incrementing_counter() {
    let versioning = on_line("cli", "alpha");

    let projects = [make_project("cli", "2.0.0", &[])];
    let intents = [make_intent("one", &[("cli", "minor")])];
    let enter_plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(enter_plan.releases[0].new_version, "2.1.0-alpha.0");

    let projects = [make_project("cli", "2.1.0-alpha.0", &[])];
    let intents =
        [make_intent("one", &[("cli", "minor")]), make_intent("two", &[("cli", "patch")])];
    let consumed = ledger(&[("cli@2.1.0-alpha.0", &["one"])]);
    let next_plan = assemble(&projects, &intents, &consumed, Some(&versioning));
    assert_eq!(next_plan.releases[0].new_version, "2.1.0-alpha.1");
}

#[test]
fn a_bigger_bump_landing_later_escalates_the_stable_target_of_the_prerelease_line() {
    let projects = [make_project("cli", "2.1.0-alpha.1", &[])];
    let intents =
        [make_intent("one", &[("cli", "minor")]), make_intent("two", &[("cli", "major")])];
    let consumed = ledger(&[("cli@2.1.0-alpha.0", &["one"]), ("cli@2.1.0-alpha.1", &[])]);
    let plan = assemble(&projects, &intents, &consumed, Some(&on_line("cli", "alpha")));
    assert_eq!(plan.releases[0].new_version, "3.0.0-alpha.0");
}

#[test]
fn packages_off_the_line_release_stable_versions_from_the_same_run() {
    let projects = [make_project("cli", "2.0.0", &[]), make_project("lib", "1.0.0", &[])];
    let intents = [make_intent("one", &[("cli", "minor"), ("lib", "minor")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&on_line("cli", "alpha")));
    assert_eq!(release(&plan, "cli").new_version, "2.1.0-alpha.0");
    assert_eq!(release(&plan, "lib").new_version, "1.1.0");
}

#[test]
fn exiting_the_line_releases_the_accumulated_stable_version_even_without_pending_intents() {
    let projects = [make_project("cli", "2.1.0-alpha.2", &[])];
    let intents =
        [make_intent("one", &[("cli", "minor")]), make_intent("two", &[("cli", "patch")])];
    let consumed = ledger(&[("cli@2.1.0-alpha.0", &["one"]), ("cli@2.1.0-alpha.2", &["two"])]);
    let plan = assemble(&projects, &intents, &consumed, None);
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].new_version, "2.1.0");
    let mut consumed_ids: Vec<&str> =
        plan.releases[0].intents.iter().map(|intent| intent.id.as_str()).collect();
    consumed_ids.sort_unstable();
    assert_eq!(consumed_ids, ["one", "two"]);
}

#[test]
fn an_intent_naming_a_stable_and_a_line_package_is_consumed_half_by_half() {
    let projects = [make_project("cli", "2.0.0", &[]), make_project("lib", "1.0.1", &[])];
    let intents = [make_intent("one", &[("cli", "minor"), ("lib", "patch")])];
    let consumed = ledger(&[("lib@1.0.1", &["one"])]);
    let plan = assemble(&projects, &intents, &consumed, Some(&on_line("cli", "alpha")));
    assert_eq!(release_names(&plan), ["cli"]);
    assert_eq!(plan.releases[0].new_version, "2.1.0-alpha.0");
}

#[test]
fn snapshot_plans_release_the_same_set_under_snapshot_versions() {
    let projects = [
        make_project("lib", "1.0.0", &[]),
        make_project("cli", "1.0.0", &[("lib", "workspace:*")]),
    ];
    let intents = [make_intent("one", &[("lib", "patch")])];
    let opts = AssembleReleasePlanOptions {
        snapshot_suffix: Some("preview-20260712000000".to_string()),
        ..AssembleReleasePlanOptions::default()
    };
    let plan = assemble_release_plan(&projects, &intents, &Ledger::new(), None, &opts)
        .expect("plan assembles");
    let versions: Vec<&str> =
        plan.releases.iter().map(|release| release.new_version.as_str()).collect();
    assert_eq!(versions, ["0.0.0-preview-20260712000000", "0.0.0-preview-20260712000000"]);
}

#[test]
fn filter_narrows_the_plan_to_the_selection_plus_companions_and_invalidated_dependents() {
    let projects = [
        make_project("lib", "1.0.0", &[]),
        make_project("cli", "1.0.0", &[("lib", "workspace:*")]),
        make_project("unrelated", "1.0.0", &[]),
    ];
    let intents =
        [make_intent("one", &[("lib", "patch")]), make_intent("two", &[("unrelated", "major")])];
    let opts = AssembleReleasePlanOptions {
        filter: Some(HashSet::from(["lib".to_string()])),
        ..AssembleReleasePlanOptions::default()
    };
    let plan = assemble_release_plan(&projects, &intents, &Ledger::new(), None, &opts)
        .expect("plan assembles");
    assert_eq!(release_names(&plan), ["cli", "lib"]);
}

#[test]
fn materialize_workspace_range_mirrors_pack_time_materialization() {
    assert_eq!(materialize_workspace_range("workspace:*", "1.2.3").as_deref(), Some("1.2.3"));
    assert_eq!(materialize_workspace_range("workspace:^", "1.2.3").as_deref(), Some("^1.2.3"));
    assert_eq!(materialize_workspace_range("workspace:~", "1.2.3").as_deref(), Some("~1.2.3"));
    assert_eq!(materialize_workspace_range("workspace:^1.0.0", "1.2.3").as_deref(), Some("^1.0.0"));
    assert_eq!(materialize_workspace_range("workspace:lib@^", "1.2.3").as_deref(), Some("^1.2.3"));
    assert_eq!(materialize_workspace_range("^1.0.0", "1.2.3"), None);
}
