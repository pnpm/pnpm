use super::{
    Ledger, assemble, assemble_with_unpublished, assert_eq, make_intent, make_project, on_lane,
    release,
};

#[test]
fn a_first_release_on_a_lane_whose_manifest_is_already_an_unpublished_prerelease_is_verbatim() {
    let projects = [make_project("cli", "2.0.0-alpha.3", &[])];
    let intents = [make_intent("one", &[("cli", "minor")])];
    let versioning = on_lane("cli", "alpha");
    let plan = assemble_with_unpublished(&projects, &intents, Some(&versioning), &["cli"]);
    assert_eq!(release(&plan, "cli").version.next, "2.0.0-alpha.3");
}

#[test]
fn an_npm_alias_with_space_comparators_on_a_workspace_name_stays_external() {
    let projects = [
        make_project("foo", "7.5.0", &[]),
        make_project("app", "1.0.0", &[("foo", "npm:bar@>=7.0.0 <8.0.0")]),
    ];
    let intents = [make_intent("one", &[("foo", "patch")])];
    let plan = assemble(&projects, &intents, &Ledger::new(), None);
    assert_eq!(plan.releases.len(), 1);
    assert_eq!(release(&plan, "foo").version.next, "7.5.1");
}
