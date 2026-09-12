use super::{
    AssembleReleasePlanOptions, IndexMap, Ledger, LedgerEntry, VersioningSettings, assemble,
    assemble_release_plan, assert_eq, make_intent, make_project, release_names, twins,
};

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
fn a_name_shared_by_two_projects_is_ambiguous_and_must_be_referenced_by_directory() {
    let intents = [make_intent("one", &[("pnpm", "patch")])];
    let err = assemble_release_plan(
        &twins(),
        std::path::Path::new("/ws"),
        &intents,
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect_err("plan must fail");
    assert!(err.to_string().contains("matches multiple workspace projects"), "unexpected: {err}");

    let intents = [make_intent("one", &[("./pnpm/npm/pnpm", "patch")])];
    let plan = assemble(&twins(), &intents, &Ledger::new(), None);
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(plan.releases[0].dir, "pnpm/npm/pnpm");
    assert_eq!(plan.releases[0].new_version, "12.0.1");
}

#[test]
fn ledger_consumption_attributes_by_directory_when_names_collide() {
    let intents = [make_intent("one", &[("./pnpm11/pnpm", "patch"), ("./pnpm/npm/pnpm", "patch")])];
    let mut consumed = Ledger::new();
    consumed.insert(
        "pnpm@12.0.1".to_string(),
        LedgerEntry::Attributed {
            dir: "pnpm/npm/pnpm".to_string(),
            intents: vec!["one".to_string()],
        },
    );
    let plan = assemble(&twins(), &intents, &consumed, None);
    // The Rust line already consumed the intent; only the TS line still
    // releases.
    assert_eq!(release_names(&plan), ["pnpm"]);
    assert_eq!(plan.releases[0].dir, "pnpm11/pnpm");
}

#[test]
fn lanes_keyed_by_directory_path_apply_to_the_right_twin() {
    let intents = [make_intent("one", &[("./pnpm11/pnpm", "patch"), ("./pnpm/npm/pnpm", "minor")])];
    let versioning = VersioningSettings {
        lanes: IndexMap::from([("./pnpm/npm/pnpm".to_string(), "alpha".to_string())]),
        ..VersioningSettings::default()
    };
    let plan = assemble(&twins(), &intents, &Ledger::new(), Some(&versioning));
    let ts_line = plan.releases.iter().find(|release| release.dir == "pnpm11/pnpm").expect("ts");
    let rust_line =
        plan.releases.iter().find(|release| release.dir == "pnpm/npm/pnpm").expect("rust");
    assert_eq!(ts_line.new_version, "11.0.1");
    assert_eq!(rust_line.new_version, "12.1.0-alpha.0");
}
