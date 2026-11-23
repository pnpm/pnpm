use super::{
    CWD,
    DependencyType,
    ExecutionTimeLog,
    LogEvent,
    LogLevel,
    NonDeprecatedAlternative,
    Output,
    PackageManifestLog,
    PackageManifestMessage,
    added_root,
    deprecation,
    deprecation_with_alternative,
    flush_checks,
    installed_deprecation,
    render,
    resolution_done,
    state,
    summary,
};

fn not_deprecated(version: &str) -> Option<NonDeprecatedAlternative> {
    Some(NonDeprecatedAlternative { version: version.to_string(), outside_declared_range: false })
}

fn execution_time() -> LogEvent {
    LogEvent::ExecutionTime(ExecutionTimeLog { level: LogLevel::Debug, started_at: 0, ended_at: 1 })
}

#[test]
fn added_deprecated_dependency_carries_the_warning_on_its_summary_line() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            deprecation_with_alternative("express", "0.14.1", 0, CWD, not_deprecated("4.21.2")),
            added_root("express", "0.14.1", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ express 0.14.1 deprecated. 4.21.2 is not deprecated.\n");
}

#[test]
fn footer_counts_every_deprecated_package() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            deprecation("express", "0.14.1", 0, CWD),
            deprecation("uuid", "3.4.0", 1, CWD),
            installed_deprecation("request", "2.88.2", 0),
            added_root("express", "0.14.1", DependencyType::Prod),
            summary(),
            execution_time(),
        ],
    );
    assert!(
        frame.starts_with(
            "\ndependencies:\n+ express 0.14.1 deprecated\n\n\
             ✗ 3 packages are deprecated: express@0.14.1, request@2.88.2 and uuid@3.4.0\n\n\
             Done in ",
        ),
        "{frame}",
    );
}

#[test]
fn footer_lists_three_deprecated_packages_and_counts_the_rest() {
    let mut reporter = state(false);
    let mut events: Vec<LogEvent> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|name| deprecation(name, "1.0.0", 1, CWD))
        .collect();
    events.push(execution_time());
    let frame = render(&mut reporter, events);
    assert!(
        frame.starts_with("\n✗ 5 packages are deprecated: a@1.0.0, b@1.0.0, c@1.0.0 and 2 more\n"),
        "{frame}",
    );
}

/// The deprecation and peer checks print nothing when they pass.
#[test]
fn passing_checks_print_no_footer() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![resolution_done(), execution_time()]);
    assert!(frame.starts_with("Done in "), "{frame}");
}

/// A command that fails after resolution emits no `Done in ...` line.
#[test]
fn footer_is_flushed_when_the_command_ends_without_done_line() {
    let mut reporter = state(false);
    render(&mut reporter, vec![deprecation("express", "0.14.1", 0, CWD)]);
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(frame, "\n✗ 1 package is deprecated: express@0.14.1\n");
    assert!(matches!(flush_checks(&mut reporter), Output::None));
}

/// The flush runs after every command, so with nothing held it must not draw
/// an empty frame into output such as `pnpm cat-file`.
#[test]
fn nothing_is_flushed_when_no_check_ran() {
    let mut reporter = state(false);
    assert!(matches!(flush_checks(&mut reporter), Output::None));
}

/// `pnpm add --lockfile-only` links nothing, so its summary has no section
/// until the edited manifest arrives after it.
#[test]
fn deprecation_marks_a_line_a_later_manifest_edit_adds() {
    let manifest = |updated: bool, value: serde_json::Value| {
        LogEvent::PackageManifest(PackageManifestLog {
            level: LogLevel::Debug,
            message: if updated {
                PackageManifestMessage::Updated { prefix: CWD.to_string(), updated: value }
            } else {
                PackageManifestMessage::Initial { prefix: CWD.to_string(), initial: value }
            },
        })
    };
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            manifest(false, serde_json::json!({})),
            deprecation("express", "0.14.1", 0, CWD),
            summary(),
        ],
    );
    assert_eq!(frame, "");
    let frame = render(
        &mut reporter,
        vec![manifest(true, serde_json::json!({ "dependencies": { "express": "0.14.1" } }))],
    );
    assert_eq!(frame, "\ndependencies:\n+ express 0.14.1 deprecated\n");
}

#[test]
fn every_notice_of_a_package_on_a_summary_line_is_carried_by_it() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            deprecation("left-pad", "1.0.0", 0, "/repo/packages/a"),
            deprecation("left-pad", "1.0.0", 0, CWD),
            added_root("left-pad", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ left-pad 1.0.0 deprecated\n");
}

/// After a failure, a passing verdict or a newer version is noise beside the
/// error, so only the problems print.
#[test]
fn failed_command_prints_only_the_problems() {
    let mut reporter = state(false);
    render(
        &mut reporter,
        vec![
            resolution_done(),
            deprecation("express", "0.14.1", 0, CWD),
            super::added_root_with_latest_at(
                CWD,
                "foo",
                "1.0.0",
                Some("2.0.0"),
                DependencyType::Prod,
            ),
        ],
    );
    let Output::Frame(frame) =
        reporter.flush_pending_checks(pnpm_default_reporter::state::CommandOutcome::Failed)
    else {
        panic!("the problems render");
    };
    assert_eq!(frame, "\n✗ 1 package is deprecated: express@0.14.1\n");
}
