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
