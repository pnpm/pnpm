use super::{
    CWD,
    DependencyType,
    ExecutionTimeLog,
    LogEvent,
    LogLevel,
    NonDeprecatedAlternative,
    Output,
    added_root,
    deprecation,
    deprecation_with_alternative,
    render,
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
fn unchanged_deprecated_dependency_follows_the_summary_sections() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            deprecation_with_alternative("express", "0.14.1", 0, CWD, not_deprecated("4.21.2")),
            added_root("foo", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(
        frame,
        "\ndependencies:\n+ foo 1.0.0\n\n[WARN] deprecated: express@0.14.1. 4.21.2 is not deprecated.\n",
    );
}

#[test]
fn deprecations_without_a_summary_print_with_the_footer() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![deprecation("express", "0.14.1", 0, CWD), deprecation("uuid", "3.4.0", 1, CWD)],
    );
    assert_eq!(frame, "");
    let frame = render(&mut reporter, vec![execution_time()]);
    assert!(
        frame.starts_with(
            "[WARN] deprecated: express@0.14.1\n\
             [WARN] 1 deprecated subdependencies found: uuid@3.4.0\n\
             Done in ",
        ),
        "{frame}",
    );
}

/// A command that fails after resolution emits no summary and no footer.
#[test]
fn deprecations_are_flushed_when_the_command_ends_without_a_footer() {
    let mut reporter = state(false);
    render(&mut reporter, vec![deprecation("express", "0.14.1", 0, CWD)]);
    let Output::Frame(frame) = reporter.flush_pending_deprecations() else {
        panic!("the held warning renders");
    };
    assert_eq!(frame, "[WARN] deprecated: express@0.14.1");
    assert!(matches!(reporter.flush_pending_deprecations(), Output::None));
}

#[test]
fn deprecations_the_summary_carried_are_not_repeated() {
    let mut reporter = state(false);
    render(
        &mut reporter,
        vec![
            deprecation("express", "0.14.1", 0, CWD),
            added_root("express", "0.14.1", DependencyType::Prod),
            summary(),
        ],
    );
    assert!(matches!(reporter.flush_pending_deprecations(), Output::None));
}
