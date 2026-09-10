use super::{
    CWD, package_manifest_initial_at, package_manifest_updated_at, render, state,
    state_without_summary_prefix_filter, summary,
};

#[test]
fn empty_summary_does_not_prevent_later_manifest_diff_summary() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at(CWD, serde_json::json!({})),
            summary(),
            package_manifest_updated_at(
                CWD,
                serde_json::json!({ "dependencies": { "foo": "^1.0.0" } }),
            ),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo ^1.0.0\n");
}

#[test]
fn summary_keeps_manifest_diffs_separate_when_including_all_prefixes() {
    let mut reporter = state_without_summary_prefix_filter();
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at("/global/a", serde_json::json!({})),
            package_manifest_updated_at(
                "/global/a",
                serde_json::json!({ "dependencies": { "a": "1.0.0" } }),
            ),
            package_manifest_initial_at("/global/b", serde_json::json!({})),
            package_manifest_updated_at(
                "/global/b",
                serde_json::json!({ "devDependencies": { "b": "2.0.0" } }),
            ),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ a 1.0.0\n\ndevDependencies:\n+ b 2.0.0\n");
}
