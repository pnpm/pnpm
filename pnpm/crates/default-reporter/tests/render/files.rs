use super::{ReporterOptions, ignored_scripts, render, state, state_with_options};
use pnpm_reporter::{LogEvent, LogLevel, PendingSkillsLog};

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
        "frame: {frame}"
    );
    assert!(!frame.contains("Ignored build scripts"), "frame: {frame}");
    assert!(!frame.contains("Agent skills awaiting approval"), "frame: {frame}");
}
