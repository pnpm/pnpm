use super::{
    AssembleReleasePlanOptions, Ledger, Path, VersioningSettings, assemble_release_plan,
    check_versioning_invariants, epic, make_project,
};

#[test]
fn a_package_matched_by_two_epics_is_a_configuration_error() {
    let projects = [
        make_project("pnpm", "11.0.0", &[]),
        make_project("other", "2.0.0", &[]),
        make_project("lib", "1101.0.0", &[]),
    ];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["lib"]), epic("other", &["lib"])],
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
    assert!(err.to_string().contains("at most one epic"), "unexpected error: {err}");
}

#[test]
fn a_fixed_group_straddling_an_epic_boundary_is_a_configuration_error() {
    let projects = [
        make_project("pnpm", "11.0.0", &[]),
        make_project("lib", "1101.0.0", &[]),
        make_project("outsider", "3.0.0", &[]),
    ];
    let versioning = VersioningSettings {
        epics: vec![epic("pnpm", &["lib"])],
        fixed: vec![vec!["lib".to_string(), "outsider".to_string()]],
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
    assert!(err.to_string().contains("straddles the epic"), "unexpected error: {err}");
}

#[test]
fn check_versioning_invariants_surfaces_malformed_configuration() {
    let projects = [make_project("pnpm", "11.15.1", &[])];
    let versioning = VersioningSettings {
        epics: vec![epic("ghost", &["@pnpm/lib"])],
        ..VersioningSettings::default()
    };
    let error = check_versioning_invariants(&projects, Path::new("/ws"), Some(&versioning))
        .expect_err("unknown lead must error");
    assert!(error.to_string().contains("is not a releasable workspace project"));
}
