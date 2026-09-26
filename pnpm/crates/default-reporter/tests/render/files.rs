use super::{ReporterOptions, applied_patches, ignored_scripts, render, state, state_with_options};

#[test]
fn the_ignored_builds_instruction_defaults_to_the_pnpm_command() {
    let mut reporter = state(false);

    let frame = render(&mut reporter, vec![ignored_scripts(&["esbuild"])]);

    assert!(frame.contains("Ignored build scripts: esbuild."), "frame: {frame}");
    assert!(frame.contains(r#"Run "pnpm approve-builds""#), "frame: {frame}");
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

    assert!(frame.contains("Ignored build scripts: esbuild."), "frame: {frame}");
    assert!(frame.contains("Set allowScripts in workspace.jsonc."), "frame: {frame}");
    assert!(!frame.contains("pnpm approve-builds"), "frame: {frame}");
}

#[test]
fn applied_patches_are_listed_one_per_line() {
    let mut reporter = state(false);

    let frame =
        render(&mut reporter, vec![applied_patches(&["is-negative@1.0.0", "is-positive@1.0.0"])]);

    assert!(
        frame.contains("Applied patches:\nis-negative@1.0.0 \u{2714}\nis-positive@1.0.0 \u{2714}"),
        "frame: {frame}",
    );
}
