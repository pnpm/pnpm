use super::{
    AssembleReleasePlanOptions, HashSet, IndexMap, Ledger, ReleaseCause, VersioningSettings,
    assemble, assemble_release_plan, assert_eq, epic, ledger, make_intent, make_project, on_lane,
    release, release_names,
};

#[test]
fn intents_already_recorded_in_the_ledger_are_not_consumed_again() {
    let projects = [make_project("a", "1.0.1", &[])];
    let intents = [make_intent("one", &[("a", "patch")])];
    let consumed = ledger(&[("a@1.0.1", &["one"])]);
    let plan = assemble(&projects, &intents, &consumed, None);
    assert_eq!(plan.releases.len(), 0);
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
fn an_intent_demanding_a_release_of_an_unreleasable_package_fails_the_plan() {
    let projects = [make_project("lib", "1.0.0", &[]), make_project("frozen", "1.0.0", &[])];
    let intents = [make_intent("one", &[("frozen", "patch"), ("lib", "patch")])];
    let versioning =
        VersioningSettings { ignore: vec!["frozen".to_string()], ..VersioningSettings::default() };
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
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
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("not a package in this workspace"), "unexpected error: {err}");
}

#[test]
fn an_intent_naming_a_main_lane_and_a_lane_package_is_consumed_half_by_half() {
    let projects = [make_project("cli", "2.0.0", &[]), make_project("lib", "1.0.1", &[])];
    let intents = [make_intent("one", &[("cli", "minor"), ("lib", "patch")])];
    let consumed = ledger(&[("lib@1.0.1", &["one"])]);
    let plan = assemble(&projects, &intents, &consumed, Some(&on_lane("cli", "alpha")));
    assert_eq!(release_names(&plan), ["cli"]);
    assert_eq!(plan.releases[0].new_version, "2.1.0-alpha.0");
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
    let plan = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        None,
        &opts,
    )
    .expect("plan assembles");
    assert_eq!(release_names(&plan), ["cli", "lib"]);
}

#[test]
fn a_lane_named_main_is_rejected_as_the_reserved_default_lane() {
    let projects = [make_project("cli", "2.0.0", &[])];
    let versioning = on_lane("cli", "Main");
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &[],
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("reserved default lane"), "unexpected error: {err}");
}

#[test]
fn when_the_lead_reaches_a_new_stable_major_every_member_re_bases_to_the_band_floor() {
    let projects = [
        make_project("pnpm", "11.9.9", &[]),
        make_project("lib", "1101.4.2", &[]),
        make_project("ui", "1105.0.0", &[]),
    ];
    let intents = [make_intent("one", &[("pnpm", "major")])];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["lib", "ui"])],
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "pnpm").new_version, "12.0.0");
    assert_eq!(release(&plan, "lib").new_version, "1200.0.0");
    assert_eq!(release(&plan, "lib").causes, vec![ReleaseCause::Epic]);
    assert_eq!(release(&plan, "ui").new_version, "1200.0.0");
}

#[test]
fn a_member_on_a_lane_re_bases_to_a_prerelease_of_the_band_floor() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "1101.2.0", &[])];
    let intents = [make_intent("one", &[("pnpm", "major")])];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["lib"])],
        lanes: IndexMap::from([("lib".to_string(), "alpha".to_string())]),
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "lib").new_version, "1200.0.0-alpha.0");
}

#[test]
fn the_re_base_waits_while_the_lead_is_on_a_prerelease_lane() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "1101.2.0", &[])];
    let intents = [make_intent("one", &[("pnpm", "major"), ("lib", "patch")])];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["lib"])],
        lanes: IndexMap::from([("pnpm".to_string(), "alpha".to_string())]),
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "pnpm").new_version, "12.0.0-alpha.0");
    // The member versions inside its old band until the lead's stable release.
    assert_eq!(release(&plan, "lib").new_version, "1101.2.1");
}

#[test]
fn the_members_re_base_when_a_prerelease_lead_graduates_to_its_new_major() {
    let projects =
        [make_project("pnpm", "12.0.0-alpha.0", &[]), make_project("lib", "1101.2.0", &[])];
    let intents = [make_intent("one", &[("pnpm", "major")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..Default::default() };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "pnpm").new_version, "12.0.0");
    assert_eq!(release(&plan, "lib").new_version, "1200.0.0");
}

#[test]
fn the_top_of_the_band_takes_a_minor_without_tripping_the_ceiling_guard() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "1199.4.2", &[])];
    let intents = [make_intent("one", &[("lib", "minor")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..VersioningSettings::default() };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "lib").new_version, "1199.5.0");
}
