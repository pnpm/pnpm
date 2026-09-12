use super::{assemble_with_unpublished, assert_eq, make_intent, make_project, on_lane, release};

#[test]
fn a_first_release_on_a_lane_whose_manifest_is_already_an_unpublished_prerelease_is_verbatim() {
    let projects = [make_project("cli", "2.0.0-alpha.3", &[])];
    let intents = [make_intent("one", &[("cli", "minor")])];
    let versioning = on_lane("cli", "alpha");
    let plan = assemble_with_unpublished(&projects, &intents, Some(&versioning), &["cli"]);
    assert_eq!(release(&plan, "cli").new_version, "2.0.0-alpha.3");
}
