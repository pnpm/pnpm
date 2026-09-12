use super::{
    AssembleReleasePlanOptions, DependencyUpdate, Ledger, ReleaseBumpType, assemble,
    assemble_release_plan, assert_eq, make_intent, make_project, materialize_workspace_range,
    release, release_names,
};

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
        vec![DependencyUpdate { name: "lib".to_string(), new_version: "2.0.0".to_string() }],
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
fn an_internal_dependency_without_the_workspace_protocol_fails_a_release_but_not_a_read_only_assemble()
 {
    let projects =
        [make_project("lib", "1.0.0", &[]), make_project("cli", "1.0.0", &[("lib", "^1.0.0")])];
    // enforce_workspace_protocol off (the default, used by `pnpm change
    // status`): a read-only assemble never fails on an unmigrated dependency.
    let plan = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &[],
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("read-only assemble succeeds");
    assert!(plan.releases.is_empty());
    // The release path enforces the prerequisite.
    let err = assemble_release_plan(
        &projects,
        std::path::Path::new("/ws"),
        &[],
        &Ledger::new(),
        None,
        &AssembleReleasePlanOptions {
            enforce_workspace_protocol: true,
            ..AssembleReleasePlanOptions::default()
        },
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
fn non_ascii_workspace_aliases_do_not_panic() {
    assert_eq!(
        materialize_workspace_range("workspace:\u{e9}dition@^", "1.2.3").as_deref(),
        Some("^1.2.3"),
    );
    assert_eq!(
        materialize_workspace_range("workspace:\u{e9}@1.0.0", "1.2.3").as_deref(),
        Some("1.0.0"),
    );
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
