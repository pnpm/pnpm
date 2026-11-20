use super::{
    CWD,
    DependencyType,
    LogEvent,
    LogLevel,
    RootLog,
    RootMessage,
    SummaryLog,
    added_root_at,
    added_root_with_latest_at,
    deprecation,
    package_manifest_initial_at,
    package_manifest_updated_at,
    render,
    state,
    summary,
};
use pnpm_reporter::RemovedRoot;
use serde_json::json;

fn workspace_summary(new_projects: &[&str]) -> LogEvent {
    LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        new_projects: Some(
            new_projects
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
    })
}

fn removed_root_at(prefix: &str, name: &str, version: &str) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Removed {
            prefix: prefix.to_string(),
            removed: RemovedRoot {
                name: name.to_string(),
                version: Some(version.to_string()),
                dependency_type: Some(DependencyType::Prod),
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
fn change_in_several_projects_names_three_and_counts_the_rest() {
    let mut reporter = state(false);
    let mut events: Vec<LogEvent> = ["e", "d", "c", "b", "a"]
        .iter()
        .map(|project| removed_root_at(&format!("/repo/packages/{project}"), "lodash", "4.17.21"))
        .collect();
    events.push(workspace_summary(&[]));
    let frame = render(&mut reporter, events);
    assert_eq!(
        frame,
        "\ndependencies:\n- lodash 4.17.21  packages/a, packages/b, packages/c and 2 more\n",
    );
}

#[test]
fn change_in_the_workspace_root_is_named_dot() {
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
    assert_eq!(frame, "\ndevDependencies:\n+ bar 1.0.0  packages/a\n+ foo 1.0.0  ., packages/a\n",);
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
    assert_eq!(
        frame,
        "\ndependencies:\n+ foo 1.0.0  packages/a\n+ foo 2.0.0  packages/b, packages/c\n",
    );
}

/// On a first install every project is new: plain additions are left out,
/// while deprecated and outdated dependencies and the ones the command
/// asked for are still listed.
#[test]
fn first_workspace_install_lists_only_what_carries_information() {
    let mut reporter = state(false);
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
    let warning = pnpm_default_reporter::format::zoom_out(
        CWD,
        "/repo/packages/a",
        "[WARN] deprecated old@1.0.0",
    );
    assert_eq!(
        frame,
        format!(
            "{warning}\n\
         \n\
         dependencies:\n\
         + old 1.0.0  packages/a\n\
         + wanted 3.0.0  packages/a\n\
         \n\
         devDependencies:\n\
         + stale 1.0.0 (2.0.0 is available)  packages/b\n",
        ),
    );
}

/// pnpm reports a deprecated package once, for the first project that
/// reached it.
#[test]
fn deprecated_dependency_of_a_new_project_is_listed_whoever_it_was_reported_for() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            deprecation("old", "1.0.0", 1, "/repo/packages/a"),
            added_root_at("/repo/packages/b", "old", "1.0.0", DependencyType::Prod),
            workspace_summary(&["/repo/packages/b"]),
        ],
    );
    assert!(frame.ends_with("\ndependencies:\n+ old 1.0.0  packages/b\n"), "{frame}");
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
    assert_eq!(frame, "\ndependencies:\n+ bar 1.0.0  packages/old\n+ baz 1.0.0\n");
}
