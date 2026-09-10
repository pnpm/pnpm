use super::{
    AssembleReleasePlanOptions, IndexMap, Ledger, Path, PathBuf, ReleaseBumpType,
    VersioningInvariantCode, VersioningSettings, WorkspaceProject, assemble, assemble_release_plan,
    assemble_with_unpublished, assert_eq, check_versioning_invariants, epic, ledger, make_intent,
    make_project, on_lane, project_at, release, release_names,
};

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
fn a_none_bump_type_releases_nothing() {
    let projects = [make_project("a", "1.0.0", &[])];
    let intents = [make_intent("one", &[("a", "none")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(plan.releases.len(), 0);
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
fn max_bump_rejects_a_plan_whose_effective_bump_exceeds_the_cap() {
    let projects = [make_project("lib", "1.0.0", &[])];
    let intents = [make_intent("one", &[("lib", "minor")])];
    let versioning = VersioningSettings {
        max_bump: Some(ReleaseBumpType::Patch),
        ..VersioningSettings::default()
    };
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
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
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("maxBump"), "unexpected error: {err}");
}

#[test]
fn a_package_on_a_lane_emits_tagged_versions_with_an_incrementing_counter() {
    let versioning = on_lane("cli", "alpha");

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
fn a_bigger_bump_landing_later_escalates_the_stable_target_of_the_lane() {
    let projects = [make_project("cli", "2.1.0-alpha.1", &[])];
    let intents =
        [make_intent("one", &[("cli", "minor")]), make_intent("two", &[("cli", "major")])];
    let consumed = ledger(&[("cli@2.1.0-alpha.0", &["one"]), ("cli@2.1.0-alpha.1", &[])]);
    let plan = assemble(&projects, &intents, &consumed, Some(&on_lane("cli", "alpha")));
    assert_eq!(plan.releases[0].new_version, "3.0.0-alpha.0");
}

#[test]
fn packages_on_the_main_lane_release_stable_versions_from_the_same_run() {
    let projects = [make_project("cli", "2.0.0", &[]), make_project("lib", "1.0.0", &[])];
    let intents = [make_intent("one", &[("cli", "minor"), ("lib", "minor")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&on_lane("cli", "alpha")));
    assert_eq!(release(&plan, "cli").new_version, "2.1.0-alpha.0");
    assert_eq!(release(&plan, "lib").new_version, "1.1.0");
}

#[test]
fn returning_to_the_main_lane_releases_the_accumulated_stable_version_even_without_pending_intents()
{
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
    let plan = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        None,
        &opts,
    )
    .expect("plan assembles");
    let versions: Vec<&str> =
        plan.releases.iter().map(|release| release.new_version.as_str()).collect();
    assert_eq!(versions, ["0.0.0-preview-20260712000000", "0.0.0-preview-20260712000000"]);
}

#[test]
fn two_same_named_projects_releasing_to_the_same_version_is_a_hard_error() {
    let same_version = [
        WorkspaceProject {
            root_dir: PathBuf::from("/ws/a/util"),
            name: Some("@scope/util".to_string()),
            version: Some("1.0.0".to_string()),
            prod_dependencies: Vec::new(),
        },
        WorkspaceProject {
            root_dir: PathBuf::from("/ws/b/util"),
            name: Some("@scope/util".to_string()),
            version: Some("1.0.0".to_string()),
            prod_dependencies: Vec::new(),
        },
    ];
    let intents = [make_intent("one", &[("./a/util", "patch"), ("./b/util", "patch")])];
    let err = assemble_release_plan(
        &same_version,
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(
        err.to_string().contains("Two projects both release @scope/util@1.0.1"),
        "unexpected error: {err}",
    );
}

#[test]
fn epic_members_move_independently_inside_the_band_while_the_lead_major_holds() {
    let projects = [make_project("pnpm", "11.2.0", &[]), make_project("lib", "1101.4.2", &[])];
    let intents = [make_intent("one", &[("pnpm", "patch"), ("lib", "minor")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..VersioningSettings::default() };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release(&plan, "pnpm").new_version, "11.2.1");
    assert_eq!(release(&plan, "lib").new_version, "1101.5.0");
}

#[test]
fn a_major_intent_bumps_a_member_to_the_next_major_inside_the_band() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "1101.4.2", &[])];
    let intents = [make_intent("one", &[("lib", "major")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..VersioningSettings::default() };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release_names(&plan), ["lib"]);
    assert_eq!(release(&plan, "lib").new_version, "1102.0.0");
}

#[test]
fn epic_membership_resolves_directory_globs_and_honors_negations() {
    let projects = [
        project_at("pnpm", "11.0.0", "pnpm"),
        project_at("@scope/a", "1100.0.0", "pkgs/a"),
        project_at("@scope/b", "1100.0.0", "pkgs/b"),
        project_at("@scope/tool", "5.0.0", "tools/tool"),
    ];
    let intents = [make_intent("one", &[("pnpm", "major")])];
    let versioning = VersioningSettings {
        epics: vec![epic("./pnpm", &["./pkgs/**", "!./pkgs/b"])],
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release_names(&plan), ["@scope/a", "pnpm"]);
    assert_eq!(release(&plan, "@scope/a").new_version, "1200.0.0");
}

#[test]
fn epic_selectors_are_order_dependent_a_later_include_overrides_an_earlier_negation() {
    let projects = [
        project_at("pnpm", "11.0.0", "pnpm"),
        project_at("@scope/a", "1100.0.0", "pkgs/a"),
        project_at("@scope/b", "1100.0.0", "pkgs/b"),
    ];
    let intents = [make_intent("one", &[("pnpm", "major")])];
    // "!./pkgs/b" first, then "./pkgs/**" — the later include wins, so b is a member.
    let versioning = VersioningSettings {
        epics: vec![epic("./pnpm", &["!./pkgs/b", "./pkgs/**"])],
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release_names(&plan), ["@scope/a", "@scope/b", "pnpm"]);
}

#[test]
fn a_member_major_bump_that_would_exceed_the_band_ceiling_is_rejected() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "1199.4.2", &[])];
    let intents = [make_intent("one", &[("lib", "major")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..VersioningSettings::default() };
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("band is exhausted"), "unexpected error: {err}");
}

#[test]
fn a_member_below_its_epic_band_is_rejected_when_it_releases() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "5.0.0", &[])];
    let intents = [make_intent("one", &[("lib", "patch")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..VersioningSettings::default() };
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("outside the band 1100-1199"), "unexpected error: {err}");
}

#[test]
fn an_epic_whose_lead_is_not_a_releasable_project_fails_the_plan() {
    let projects = [make_project("lib", "1101.0.0", &[])];
    let versioning = VersioningSettings {
        epics: vec![epic("ghost", &["lib"])],
        ..VersioningSettings::default()
    };
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &[],
        &Ledger::new(),
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(
        err.to_string().contains("is not a releasable workspace project"),
        "unexpected error: {err}",
    );
}

#[test]
fn a_first_release_publishes_the_current_version_verbatim_ignoring_the_intent_bump() {
    let projects = [make_project("newpkg", "1100.0.0", &[])];
    let intents = [make_intent("one", &[("newpkg", "minor")])];
    let plan = assemble_with_unpublished(&projects, &intents, None, &["newpkg"]);
    let release = release(&plan, "newpkg");
    assert_eq!(release.new_version, "1100.0.0");
    // The intent is still consumed for the changelog and the ledger.
    let intent_ids: Vec<&str> = release.intents.iter().map(|intent| intent.id.as_str()).collect();
    assert_eq!(intent_ids, ["one"]);
}

#[test]
fn a_published_current_version_bumps_normally_on_the_second_release() {
    let projects = [make_project("newpkg", "1100.0.0", &[])];
    let intents = [make_intent("one", &[("newpkg", "minor")])];
    let plan = assemble_with_unpublished(&projects, &intents, None, &[]);
    assert_eq!(release(&plan, "newpkg").new_version, "1100.1.0");
}

#[test]
fn a_first_release_does_not_propagate_to_dependents_since_its_version_does_not_move() {
    let projects = [
        make_project("lib", "1100.0.0", &[]),
        make_project("cli", "3.0.0", &[("lib", "workspace:^")]),
    ];
    let intents = [make_intent("one", &[("lib", "minor")])];
    let plan = assemble_with_unpublished(&projects, &intents, None, &["lib"]);
    assert_eq!(release(&plan, "lib").new_version, "1100.0.0");
    assert_eq!(release_names(&plan), ["lib"]);
}

#[test]
fn a_first_release_on_a_lane_debuts_at_a_prerelease_of_the_current_version() {
    let projects = [make_project("cli", "12.0.0", &[])];
    let intents = [make_intent("one", &[("cli", "minor")])];
    let versioning = on_lane("cli", "alpha");
    let plan = assemble_with_unpublished(&projects, &intents, Some(&versioning), &["cli"]);
    assert_eq!(release(&plan, "cli").new_version, "12.0.0-alpha.0");
}

#[test]
fn an_unpublished_epic_member_re_bases_to_the_new_band_floor_when_the_lead_crosses_a_major() {
    let projects = [make_project("pnpm", "11.0.0", &[]), make_project("lib", "1100.0.0", &[])];
    let intents = [make_intent("one", &[("pnpm", "major"), ("lib", "minor")])];
    let versioning =
        VersioningSettings { epics: vec![epic("pnpm", &["lib"])], ..VersioningSettings::default() };
    let plan = assemble_with_unpublished(&projects, &intents, Some(&versioning), &["lib"]);
    // Debuting verbatim at 1100.0.0 would land the member outside the new
    // 1200-1299 band, so the epic re-base to the floor supersedes it.
    assert_eq!(release(&plan, "pnpm").new_version, "12.0.0");
    assert_eq!(release(&plan, "lib").new_version, "1200.0.0");
}

#[test]
fn check_versioning_invariants_passes_when_bands_and_lockstep_hold() {
    let projects = [
        make_project("pnpm", "11.15.1", &[]),
        make_project("@pnpm/lib", "1102.0.7", &[]),
        make_project("@pnpm/exe", "11.15.1", &[]),
    ];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["@pnpm/lib"])],
        fixed: vec![vec!["pnpm".to_string(), "@pnpm/exe".to_string()]],
        ..VersioningSettings::default()
    };
    let violations =
        check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning)).unwrap();
    assert!(violations.is_empty(), "unexpected violations: {violations:?}");
}

#[test]
fn check_versioning_invariants_keeps_the_previous_band_for_a_prerelease_lead() {
    let projects =
        [make_project("pnpm", "12.0.0-alpha.1", &[]), make_project("@pnpm/lib", "1102.0.7", &[])];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["@pnpm/lib"])],
        ..VersioningSettings::default()
    };
    let violations =
        check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning)).unwrap();
    assert!(violations.is_empty(), "unexpected violations: {violations:?}");
}

#[test]
fn check_versioning_invariants_reports_an_out_of_band_member() {
    let projects = [make_project("pnpm", "11.15.1", &[]), make_project("@pnpm/lib", "5.0.0", &[])];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["@pnpm/lib"])],
        ..VersioningSettings::default()
    };
    let violations =
        check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning)).unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, VersioningInvariantCode::EpicOutOfBand);
    assert!(violations[0].message.contains("outside the band 1100-1199"));
}

#[test]
fn check_versioning_invariants_reports_a_fixed_group_out_of_lockstep() {
    let projects =
        [make_project("pnpm", "11.15.1", &[]), make_project("@pnpm/exe", "11.15.0", &[])];
    let versioning = VersioningSettings {
        fixed: vec![vec!["pnpm".to_string(), "@pnpm/exe".to_string()]],
        ..VersioningSettings::default()
    };
    let violations =
        check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning)).unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, VersioningInvariantCode::FixedGroupMismatch);
    assert!(violations[0].message.contains("not in lockstep"));
}

#[test]
fn check_versioning_invariants_reports_every_violation_at_once() {
    let projects = [
        make_project("pnpm", "11.15.1", &[]),
        make_project("@pnpm/lib", "5.0.0", &[]),
        make_project("@pnpm/exe", "11.15.0", &[]),
    ];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["@pnpm/lib"])],
        fixed: vec![vec!["pnpm".to_string(), "@pnpm/exe".to_string()]],
        ..VersioningSettings::default()
    };
    let violations =
        check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning)).unwrap();
    let mut codes: Vec<_> = violations.iter().map(|violation| violation.code).collect();
    codes.sort_by_key(|code| format!("{code:?}"));
    assert_eq!(
        codes,
        [VersioningInvariantCode::EpicOutOfBand, VersioningInvariantCode::FixedGroupMismatch],
    );
}

#[test]
fn check_versioning_invariants_rejects_the_reserved_main_lane() {
    let projects = [make_project("pnpm", "11.15.1", &[])];
    let versioning = VersioningSettings {
        lanes: IndexMap::from([("pnpm".to_string(), "main".to_string())]),
        ..VersioningSettings::default()
    };
    let error = check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning))
        .expect_err("the reserved lane name must error");
    let message = error.to_string();
    eprintln!("ERROR:\n{message}\n");
    assert!(message.contains("reserved default lane"));
}

#[test]
fn check_versioning_invariants_rejects_a_fixed_group_split_across_lanes() {
    let projects =
        [make_project("pnpm", "11.15.1", &[]), make_project("@pnpm/exe", "11.15.1", &[])];
    let versioning = VersioningSettings {
        fixed: vec![vec!["pnpm".to_string(), "@pnpm/exe".to_string()]],
        lanes: IndexMap::from([("pnpm".to_string(), "alpha".to_string())]),
        ..VersioningSettings::default()
    };
    let error = check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning))
        .expect_err("fixed groups must share a lane");
    let message = error.to_string();
    eprintln!("ERROR:\n{message}\n");
    assert!(message.contains("mixes packages on different lanes"));
}

#[test]
fn negative_only_epic_selectors_do_not_include_other_packages() {
    let projects = [
        project_at("pnpm", "11.0.0", "pnpm"),
        project_at("@scope/a", "1100.0.0", "pkgs/a"),
        project_at("@scope/b", "1100.0.0", "pkgs/b"),
    ];
    let intents = [make_intent("one", &[("pnpm", "major")])];
    let versioning = VersioningSettings {
        epics: vec![epic("./pnpm", &["!./pkgs/b"])],
        ..VersioningSettings::default()
    };
    let plan = assemble(&projects, &intents, &Ledger::new(), Some(&versioning));
    assert_eq!(release_names(&plan), ["pnpm"]);
}
