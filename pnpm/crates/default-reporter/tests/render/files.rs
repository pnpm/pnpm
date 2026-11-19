use super::{ReporterOptions, ignored_scripts, render, state, state_with_options};
use pnpm_default_reporter::state::Output;
use pnpm_reporter::{ExecutionTimeLog, LogEvent, LogLevel, PendingSkillsLog};

#[test]
fn the_ignored_builds_instruction_defaults_to_the_pnpm_command() {
    let mut reporter = state(false);

    let frame = render(&mut reporter, vec![ignored_scripts(&["esbuild"])]);

    assert!(frame.contains("Packages awaiting approval:\n  esbuild  build"), "frame: {frame}");
    assert!(frame.contains(r#"Run "pnpm approve""#), "frame: {frame}");
}

/// An embedder whose users approve builds through its own configuration
/// replaces the instruction line; the list of blocked packages above it
/// is unchanged.
#[test]
fn the_ignored_builds_instruction_can_be_replaced() {
    let mut reporter = state_with_options(ReporterOptions {
        ignored_builds_instruction_text: Some("Set allowScripts in workspace.jsonc.".to_string()),
        ..ReporterOptions::default()
    });

    let frame = render(&mut reporter, vec![ignored_scripts(&["esbuild"])]);

    assert!(frame.contains("Packages awaiting approval:\n  esbuild  build"), "frame: {frame}");
    assert!(frame.contains("Set allowScripts in workspace.jsonc."), "frame: {frame}");
    assert!(!frame.contains("pnpm approve-builds"), "frame: {frame}");
}

#[test]
fn the_pending_approval_notice_combines_builds_and_skills_by_package() {
    let mut reporter = state(false);

    let frame = render(
        &mut reporter,
        vec![
            ignored_scripts(&["esbuild@1.0.0", "drizzle-kit@2.0.0"]),
            LogEvent::PendingSkills(PendingSkillsLog {
                level: LogLevel::Debug,
                package_names: vec!["drizzle-kit".to_string(), "agent-tool".to_string()],
            }),
        ],
    );

    assert!(
        frame.contains(
            "Packages awaiting approval:\n  agent-tool  skills\n  drizzle-kit  build, skills\n  esbuild  build"
        ),
        "frame: {frame}",
    );
    assert!(!frame.contains("Ignored build scripts"), "frame: {frame}");
    assert!(!frame.contains("Agent skills awaiting approval"), "frame: {frame}");
}

#[test]
fn the_pending_approval_notice_combines_source_package_labels() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            ignored_scripts(&["my-skills@link:packages/my-skills"]),
            LogEvent::PendingSkills(PendingSkillsLog {
                level: LogLevel::Debug,
                package_names: vec!["my-skills (local)".to_string()],
            }),
        ],
    );

    assert!(frame.contains("  my-skills (local)  build, skills"), "frame: {frame}");
    assert_eq!(frame.matches("my-skills (local)").count(), 1, "frame: {frame}");
}

#[test]
fn registry_skill_labels_do_not_merge_into_local_build_labels() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            ignored_scripts(&["foo@link:../foo"]),
            LogEvent::PendingSkills(PendingSkillsLog {
                level: LogLevel::Debug,
                package_names: vec!["foo".to_string()],
            }),
        ],
    );

    assert!(frame.contains("  foo  skills"), "frame: {frame}");
    assert!(frame.contains("  foo (local)  build"), "frame: {frame}");
}

#[test]
fn append_only_output_emits_one_combined_pending_approval_notice() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let output = appended_output(
        &mut reporter,
        &[
            ignored_scripts(&["esbuild@1.0.0"]),
            LogEvent::PendingSkills(PendingSkillsLog {
                level: LogLevel::Debug,
                package_names: vec!["esbuild".to_string()],
            }),
            execution_time(),
        ],
    );

    assert_eq!(output.matches("Packages awaiting approval:").count(), 1, "{output}");
    assert!(output.contains("  esbuild  build, skills"), "{output}");
}

/// One command can run several installs, such as one per `pnpm add -g`
/// group. Each reports its own packages, and the notice lists all of them.
#[test]
fn the_pending_approval_notice_keeps_packages_from_every_event() {
    let events = || {
        vec![
            ignored_scripts(&["esbuild@1.0.0"]),
            ignored_scripts(&["sharp@2.0.0"]),
            LogEvent::PendingSkills(PendingSkillsLog {
                level: LogLevel::Debug,
                package_names: vec!["agent-tool".to_string()],
            }),
            LogEvent::PendingSkills(PendingSkillsLog {
                level: LogLevel::Debug,
                package_names: vec!["sharp".to_string()],
            }),
        ]
    };
    let expected = "Packages awaiting approval:\n  agent-tool  skills\n  esbuild  build\n  sharp  build, skills";

    let frame = render(&mut state(false), events());
    assert!(frame.contains(expected), "frame: {frame}");
    assert_eq!(frame.matches("Packages awaiting approval:").count(), 1, "frame: {frame}");

    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let mut append_only_events = events();
    append_only_events.push(execution_time());
    let output = appended_output(&mut reporter, &append_only_events);
    assert!(output.contains(expected), "{output}");
    assert_eq!(output.matches("Packages awaiting approval:").count(), 1, "{output}");
}

fn execution_time() -> LogEvent {
    LogEvent::ExecutionTime(ExecutionTimeLog { level: LogLevel::Debug, started_at: 0, ended_at: 1 })
}

fn appended_output(reporter: &mut super::ReporterState, events: &[LogEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match reporter.handle(event) {
            Output::Lines(lines) => Some(lines.join("\n")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn append_only_output_flushes_pending_approvals_before_a_failed_lifecycle_script() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let _ = reporter.handle(&ignored_scripts(&["esbuild@1.0.0"]));
    let _ = reporter.handle(&LogEvent::PendingSkills(PendingSkillsLog {
        level: LogLevel::Debug,
        package_names: vec!["esbuild".to_string()],
    }));
    let Output::Lines(lines) = reporter.handle(&super::lifecycle_exit("/repo", "postinstall", 1))
    else {
        panic!("expected appended lines");
    };
    let output = lines.join("\n");

    assert_eq!(output.matches("Packages awaiting approval:").count(), 1, "{output}");
    assert!(output.contains("  esbuild  build, skills"), "{output}");
    assert!(output.contains("Failed"), "{output}");
}

#[test]
fn append_only_output_emits_approval_notice_once_for_multiple_failed_scripts() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let _ = reporter.handle(&ignored_scripts(&["esbuild@1.0.0"]));
    let _ = reporter.handle(&LogEvent::PendingSkills(PendingSkillsLog {
        level: LogLevel::Debug,
        package_names: vec!["esbuild".to_string()],
    }));
    let first = reporter.handle(&super::lifecycle_exit("/repo/packages/first", "postinstall", 1));
    let second = reporter.handle(&super::lifecycle_exit("/repo/packages/second", "postinstall", 1));

    let Output::Lines(first) = first else { panic!("expected first appended lines") };
    let Output::Lines(second) = second else { panic!("expected second appended lines") };
    assert_eq!(
        first
            .join("\n")
            .matches("Packages awaiting approval:")
            .count(),
        1,
    );
    assert!(!second.join("\n").contains("Packages awaiting approval"));
}
