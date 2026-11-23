use super::{
    CWD,
    DependencyType,
    LogEvent,
    LogLevel,
    Output,
    ReporterOptions,
    RootLog,
    RootMessage,
    SummaryLog,
    added_root_at,
    added_root_with_latest_at,
    deprecation,
    flush_checks,
    package_manifest_initial_at,
    package_manifest_updated_at,
    removed_root_at,
    render,
    state,
    state_with_options,
    summary,
};
use pnpm_reporter::AddedRoot;
use serde_json::json;

/// A reporter for a command that edits manifests, such as `pnpm add`.
fn editing_state() -> pnpm_default_reporter::state::ReporterState {
    state_with_options(ReporterOptions {
        scope: pnpm_default_reporter::state::ScopeOptions {
            edits_manifests: true,
            ..Default::default()
        },
        ..ReporterOptions::default()
    })
}

fn workspace_summary(new_projects: &[&str]) -> LogEvent {
    named_workspace_summary(new_projects, &[])
}

fn named_workspace_summary(new_projects: &[&str], project_names: &[(&str, &str)]) -> LogEvent {
    LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        new_projects: Some(
            new_projects
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
        project_names: Some(
            project_names
                .iter()
                .map(|(prefix, name)| ((*prefix).to_string(), (*name).to_string()))
                .collect(),
        ),
    })
}

fn aliased_root_at(prefix: &str, alias: &str, real_name: &str, version: &str) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: prefix.to_string(),
            added: AddedRoot {
                name: alias.to_string(),
                real_name: real_name.to_string(),
                version: Some(version.to_string()),
                dependency_type: Some(DependencyType::Prod),
                id: None,
                latest: None,
                linked_from: None,
            },
        },
    })
}

#[test]
fn single_project_summary_leaves_out_other_projects() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at(CWD, "foo", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/a", "bar", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}

#[test]
fn change_only_in_the_current_project_names_no_project() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at(CWD, "foo", "1.0.0", DependencyType::Prod),
            removed_root_at(CWD, "bar", "2.0.0"),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n- bar 2.0.0\n+ foo 1.0.0\n");
}

#[test]
fn change_in_several_projects_counts_them() {
    let mut reporter = state(false);
    let mut events: Vec<LogEvent> = ["e", "d", "c", "b", "a"]
        .iter()
        .map(|project| removed_root_at(&format!("/repo/packages/{project}"), "lodash", "4.17.21"))
        .collect();
    events.push(removed_root_at(CWD, "ms", "2.0.0"));
    events.push(workspace_summary(&[]));
    let frame = render(&mut reporter, events);
    assert_eq!(frame, "\ndependencies:\n- lodash 4.17.21  in 5 projects\n- ms 2.0.0\n");
}

#[test]
fn change_in_the_workspace_root_and_another_project_counts_both() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at(CWD, "foo", "1.0.0", DependencyType::Dev),
            added_root_at("/repo/packages/a", "foo", "1.0.0", DependencyType::Dev),
            added_root_at("/repo/packages/a", "bar", "1.0.0", DependencyType::Dev),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(
        frame,
        "\ndevDependencies:\n+ bar 1.0.0  in packages/a\n+ foo 1.0.0  in 2 projects\n",
    );
}

#[test]
fn different_versions_of_a_dependency_are_separate_lines() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at("/repo/packages/a", "foo", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/b", "foo", "2.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/c", "foo", "2.0.0", DependencyType::Prod),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0  in packages/a\n+ foo 2.0.0  in 2 projects\n");
}

#[test]
fn first_workspace_install_lists_only_what_carries_information() {
    let mut reporter = editing_state();
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at(
                "/repo/packages/a",
                json!({ "dependencies": { "plain": "^1.0.0", "old": "^1.0.0" } }),
            ),
            deprecation("old", "1.0.0", 0, "/repo/packages/a"),
            added_root_at(CWD, "plain", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/a", "plain", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/a", "old", "1.0.0", DependencyType::Prod),
            added_root_with_latest_at(
                "/repo/packages/b",
                "stale",
                "1.0.0",
                Some("2.0.0"),
                DependencyType::Dev,
            ),
            added_root_with_latest_at(CWD, "fresh", "2.0.0", Some("2.0.0"), DependencyType::Dev),
            added_root_at("/repo/packages/a", "wanted", "3.0.0", DependencyType::Prod),
            workspace_summary(&[CWD, "/repo/packages/a", "/repo/packages/b"]),
            package_manifest_updated_at(
                "/repo/packages/a",
                json!({ "dependencies": { "wanted": "^3.0.0" } }),
            ),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ wanted 3.0.0\n");
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert!(
        frame.ends_with(
            "\n✗ 1 package is deprecated: old@1.0.0\n\
             · 1 dependency has a newer version: stale (see \"pnpm outdated\")\n",
        ),
        "{frame}",
    );
}

#[test]
fn first_install_of_a_workspace_with_nothing_notable_prints_no_list() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at(CWD, "foo", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/a", "bar", "1.0.0", DependencyType::Prod),
            workspace_summary(&[CWD, "/repo/packages/a"]),
        ],
    );
    assert_eq!(frame, "");
}

#[test]
fn newly_added_project_lists_only_the_installed_projects_changes() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at("/repo/packages/new", "foo", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/new", "bar", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/old", "bar", "1.0.0", DependencyType::Prod),
            added_root_at(CWD, "baz", "1.0.0", DependencyType::Prod),
            workspace_summary(&["/repo/packages/new"]),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ bar 1.0.0  in packages/old\n+ baz 1.0.0\n");
}

#[test]
fn same_alias_for_different_packages_is_separate_lines() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            aliased_root_at("/repo/packages/a", "compat", "foo", "1.0.0"),
            aliased_root_at("/repo/packages/b", "compat", "bar", "1.0.0"),
            aliased_root_at("/repo/packages/c", "compat", "bar", "1.0.0"),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(
        frame,
        "\ndependencies:\n\
         + compat <- bar 1.0.0  in 2 projects\n\
         + compat <- foo 1.0.0  in packages/a\n",
    );
}

/// The manifest after the edit arrives once another project's change has
/// rendered the summary.
#[test]
fn new_project_dependency_moved_out_of_its_declared_range_is_listed() {
    let mut reporter = editing_state();
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at(
                "/repo/packages/a",
                json!({ "dependencies": { "foo": "^1.0.0", "bar": "^1.0.0" } }),
            ),
            added_root_at("/repo/packages/a", "foo", "2.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/a", "bar", "1.2.0", DependencyType::Prod),
            added_root_at("/repo/packages/b", "baz", "1.0.0", DependencyType::Prod),
            workspace_summary(&["/repo/packages/a"]),
            package_manifest_updated_at(
                "/repo/packages/a",
                json!({ "dependencies": { "foo": "^2.0.0", "bar": "^1.0.0" } }),
            ),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ baz 1.0.0  in packages/b\n+ foo 2.0.0  in packages/a\n");
}

#[test]
fn project_paths_lose_control_characters() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at(CWD, "bar", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/evil\n\u{1b}[2Jx", "foo", "1.0.0", DependencyType::Prod),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ bar 1.0.0\n+ foo 1.0.0  in packages/evil[2Jx\n");
}

/// An override can install a version outside the declared range without
/// the command asking for it.
#[test]
fn install_infers_no_requested_dependencies() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at(CWD, json!({ "dependencies": { "foo": "^1.0.0" } })),
            added_root_at(CWD, "foo", "2.0.0", DependencyType::Prod),
            added_root_at(CWD, "hooked", "1.0.0", DependencyType::Prod),
            workspace_summary(&[CWD]),
        ],
    );
    assert_eq!(frame, "");
}

/// A deprecated package changed in several projects is one summary line, and
/// no separate warning repeats it. One that did not change this install
/// follows the sections, with the projects it was reported for.
#[test]
fn workspace_deprecations_join_the_checks_footer() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            super::deprecation_with_alternative(
                "request",
                "2.88.2",
                0,
                "/repo/packages/api",
                Some(super::NonDeprecatedAlternative {
                    version: "3.0.0".to_string(),
                    outside_declared_range: false,
                }),
            ),
            deprecation("left-pad", "1.0.0", 0, "/repo/packages/web"),
            deprecation("left-pad", "1.0.0", 0, "/repo/packages/cli"),
            deprecation("uuid", "3.4.0", 2, "/repo/packages/api"),
            added_root_at("/repo/packages/api", "request", "2.88.2", DependencyType::Prod),
            added_root_at("/repo/packages/web", "request", "2.88.2", DependencyType::Prod),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ request 2.88.2\n");
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert!(
        frame.ends_with(
            "\n✗ 3 packages are deprecated: left-pad@1.0.0, request@2.88.2 and uuid@3.4.0\n",
        ),
        "{frame}",
    );
}

/// When every change was made by the same projects, naming them tells no
/// line apart.
#[test]
fn changes_made_by_the_same_projects_name_none() {
    let mut reporter = state(false);
    let mut events = Vec::new();
    for project in ["a", "b"] {
        let prefix = format!("/repo/packages/{project}");
        events.push(added_root_at(&prefix, "foo", "1.0.0", DependencyType::Prod));
        events.push(added_root_at(&prefix, "bar", "1.0.0", DependencyType::Dev));
    }
    events.push(workspace_summary(&[]));
    let frame = render(&mut reporter, events);
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n\ndevDependencies:\n+ bar 1.0.0\n");
}

#[test]
fn update_in_several_projects_is_one_line() {
    let mut reporter = state(false);
    let mut events = Vec::new();
    for project in ["a", "b", "c"] {
        let prefix = format!("/repo/packages/{project}");
        events.push(removed_root_at(&prefix, "lodash", "4.17.20"));
        events.push(added_root_at(&prefix, "lodash", "4.18.1", DependencyType::Prod));
    }
    events.push(workspace_summary(&[]));
    let frame = render(&mut reporter, events);
    assert_eq!(frame, "\ndependencies:\n↑ lodash 4.17.20 → 4.18.1\n");
}

#[test]
fn updates_to_different_versions_are_separate_lines() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            removed_root_at("/repo/packages/a", "lodash", "4.18.1"),
            added_root_at("/repo/packages/a", "lodash", "4.17.21", DependencyType::Prod),
            removed_root_at("/repo/packages/b", "lodash", "4.18.1"),
            added_root_at("/repo/packages/b", "lodash", "4.17.20", DependencyType::Prod),
            removed_root_at("/repo/packages/c", "lodash", "4.18.1"),
            added_root_at("/repo/packages/c", "lodash", "4.17.20", DependencyType::Prod),
            workspace_summary(&[]),
        ],
    );
    assert_eq!(
        frame,
        "\ndependencies:\n\
         ↓ lodash 4.18.1 → 4.17.20  in 2 projects\n\
         ↓ lodash 4.18.1 → 4.17.21  in packages/a\n",
    );
}

#[test]
fn change_in_one_project_names_its_package() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at(CWD, "bar", "1.0.0", DependencyType::Prod),
            added_root_at("/repo/packages/a", "foo", "1.0.0", DependencyType::Prod),
            named_workspace_summary(&[], &[("/repo/packages/a", "@scope/a")]),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ bar 1.0.0\n+ foo 1.0.0  in @scope/a\n");
}
