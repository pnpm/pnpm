use super::{
    DependencyType, ReporterOptions, Stage, added_root, added_root_at, package_manifest_initial_at,
    package_manifest_updated_at, progress_at, render, stage_at, state, state_with_options,
    state_without_summary_prefix_filter, summary,
};

#[test]
fn prints_progress_beginning_for_node_modules_outside_cwd() {
    let requester = "/repo/foo";
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![stage_at(requester, Stage::ResolutionStarted), progress_at(requester, "resolved")],
    );
    assert_eq!(
        frame,
        "foo                                      | Progress: resolved 1, reused 0, downloaded 0, added 0",
    );
}

#[test]
fn hides_progress_prefix_for_node_modules_outside_cwd() {
    let requester = "/repo/foo";
    let mut reporter = state_with_options(ReporterOptions {
        hide_progress_prefix: true,
        ..ReporterOptions::default()
    });
    let frame = render(
        &mut reporter,
        vec![stage_at(requester, Stage::ResolutionStarted), progress_at(requester, "resolved")],
    );
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 0");
}

#[test]
fn summary_ignores_root_events_outside_current_prefix() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at("/repo/packages/foo", "extra", "1.0.0", DependencyType::Prod),
            added_root("foo", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}

#[test]
fn summary_ignores_manifest_events_outside_current_prefix() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at("/repo/packages/foo", serde_json::json!({})),
            package_manifest_updated_at(
                "/repo/packages/foo",
                serde_json::json!({ "dependencies": { "extra": "^1.0.0" } }),
            ),
            summary(),
        ],
    );
    assert_eq!(frame, "");
}

#[test]
fn summary_can_include_events_outside_current_prefix() {
    let mut reporter = state_without_summary_prefix_filter();
    let frame = render(
        &mut reporter,
        vec![
            added_root_at("/global/pnpm/packages/foo", "foo", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}
