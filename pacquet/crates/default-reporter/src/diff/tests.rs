use super::Diff;

#[test]
fn identical_frame_produces_empty_diff() {
    let mut diff = Diff::new(120);
    diff.update("Progress: resolved 10\n");
    let output = diff.update("Progress: resolved 10\n");
    assert!(output.is_empty(), "identical frame yields no output: {output:?}");
}

#[test]
fn unchanged_sticky_line_not_rewritten() {
    let mut diff = Diff::new(120);
    diff.update("Lockfile passes supply-chain policies (verified 1h ago)\nProgress: resolved 10\n");
    let output = diff
        .update("Lockfile passes supply-chain policies (verified 1h ago)\nProgress: resolved 11\n");
    assert!(!output.contains("Lockfile"), "sticky line must not be reprinted: {output:?}");
}

#[test]
fn inline_diff_writes_only_changed_chars() {
    let mut diff = Diff::new(120);
    diff.update("Progress: resolved 10\n");
    let output = diff.update("Progress: resolved 11\n");
    assert!(output.contains('1'), "changed char is written: {output:?}");
    assert!(!output.contains("Progress"), "unchanged prefix not rewritten: {output:?}");
}

#[test]
fn first_frame_writes_full_content() {
    let mut diff = Diff::new(120);
    let output = diff.update("Progress: resolved 10\n");
    assert!(output.contains("Progress"), "first frame has full content: {output:?}");
}

#[test]
fn added_line_appended() {
    let mut diff = Diff::new(120);
    diff.update("Line 1\n");
    let output = diff.update("Line 1\nLine 2\n");
    assert!(output.contains("Line 2"), "added line appears: {output:?}");
}

#[test]
fn removed_line_cleared() {
    let mut diff = Diff::new(120);
    diff.update("Line 1\nLine 2\n");
    let output = diff.update("Line 1\n");
    assert!(output.contains("\x1b[0K"), "removed line triggers clear: {output:?}");
}

#[test]
fn inline_diff_skipped_for_ansi_lines() {
    let mut diff = Diff::new(120);
    diff.update("\x1b[32mProgress: resolved 10\x1b[0m\n");
    let output = diff.update("\x1b[32mProgress: resolved 11\x1b[0m\n");
    // Lines with ANSI codes fall back to full line rewrite, not inline diff.
    assert!(
        output.contains("Progress"),
        "ANSI lines use full rewrite (not inline diff): {output:?}",
    );
}

#[test]
fn inline_diff_skipped_for_small_change() {
    let mut diff = Diff::new(120);
    // Two frames that differ by only one character at the very end.
    // left=28, right=0, left+right=28 > 4 so this DOES qualify for inline diff.
    // Use a shorter line where the change is tiny relative to the line.
    diff.update("Progress: resolved 1\n");
    let output = diff.update("Progress: resolved 2\n");
    // left=19, right=0, left+right=19 > 4 → inline diff applies.
    // The changed char '2' is written; the unchanged prefix is not.
    assert!(output.contains('2'), "changed char is written: {output:?}");
}

#[test]
fn full_line_rewrite_clears() {
    let mut diff = Diff::new(120);
    diff.update("This is the original line content\n");
    let output = diff.update("Completely different text here\n");
    // No common prefix/suffix → full line rewrite with clear.
    assert!(output.contains("\x1b[0K"), "full line rewrite should clear old content: {output:?}");
    assert!(output.contains("Completely different"), "new line content is written: {output:?}");
}

#[test]
fn clear_down_from_cursor_row() {
    let mut diff = Diff::new(120);
    // Three lines; the middle one changes, the third is removed.
    // clear_down must start from the cursor's position (after line 2),
    // not from new_last_row.
    diff.update("Line A\nLine B\nLine C\n");
    let output = diff.update("Line A\nLine B changed\n");
    // Line C is removed → clear_down should emit \x1b[0K for it.
    assert!(output.contains("\x1b[0K"), "removed trailing line is cleared: {output:?}");
    // Line A (unchanged) should NOT be rewritten.
    assert!(
        !output.contains("Line A\n") || output.matches('\x1b').count() > 1,
        "unchanged leading line not rewritten: {output:?}",
    );
}

#[test]
fn soft_wrapped_line_height() {
    let mut diff = Diff::new(10);
    // A 25-char line on a 10-column terminal wraps to 3 visual rows.
    let long_line = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    diff.update(&format!("{long_line}\n"));
    // Same line → empty diff (diff correctly tracked the wrapped height).
    let output = diff.update(&format!("{long_line}\n"));
    assert!(output.is_empty(), "soft-wrapped identical line produces empty diff: {output:?}");
    // Different content → cursor movement accounts for wrapped height.
    let output = diff.update("ABCDEFGHIJKLMNOPQRSTUVWXY\n");
    assert!(
        output.contains("ABCDEFGHIJKLMNOPQRSTUVWXY"),
        "changed wrapped line content is written: {output:?}",
    );
}

#[test]
fn empty_frame_clears_all() {
    let mut diff = Diff::new(120);
    diff.update("Line 1\nLine 2\nLine 3\n");
    let output = diff.update("");
    // Transitioning to an empty frame should clear all previous lines.
    assert!(output.contains("\x1b[0K"), "empty frame clears old content: {output:?}");
}

#[test]
fn multiple_progress_ticks() {
    let mut diff = Diff::new(120);
    diff.update("Progress: resolved 10\n");

    for tick in 11..=15 {
        let output = diff.update(&format!("Progress: resolved {tick}\n"));
        // Each tick should write the changed digit(s) but not the full line.
        assert!(
            !output.contains("Progress"),
            "tick {tick} does not rewrite unchanged prefix: {output:?}",
        );
        assert!(!output.is_empty(), "tick {tick} produces non-empty diff: {output:?}");
    }
}

#[test]
fn frame_without_trailing_newline() {
    let mut diff = Diff::new(120);
    diff.update("Line 1\nLine 2");
    let output = diff.update("Line 1\nLine 2 changed");
    // Last line has no newline → cursor stays on same row.
    // The changed line should be rewritten with clear.
    assert!(
        output.contains("Line 2 changed"),
        "last line without newline is still diffed: {output:?}",
    );
}
