use pnpm_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogLevel, ProgressLog, ProgressMessage,
    PromptAction, StatsLog, StatsMessage,
};

use super::{LogEvent, Output, Sink, is_coalesceable};

#[test]
fn progress_and_in_progress_downloads_coalesce() {
    let progress = LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Resolved {
            package_id: "foo".to_string(),
            requester: "/repo".to_string(),
        },
    });
    let downloading = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::InProgress {
            downloaded: 1,
            package_id: "foo".to_string(),
        },
    });
    assert!(is_coalesceable(&progress));
    assert!(is_coalesceable(&downloading));
}

#[test]
fn stats_are_not_throttled() {
    let stats = LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: "/repo".to_string(), added: 1 },
    });
    assert!(!is_coalesceable(&stats));
}

#[test]
fn prompt_holds_redraws_and_resets_before_resuming() {
    let mut sink = Sink::new();
    let mut writes = Vec::new();

    sink.write_to(Output::Frame("before".to_string()), false, &mut writes);
    let before_prompt = writes.len();

    sink.on_prompt_to(PromptAction::Start, &mut writes);
    sink.write_to(Output::Frame("during".to_string()), false, &mut writes);
    assert_eq!(writes.len(), before_prompt);

    sink.on_prompt_to(PromptAction::End, &mut writes);
    assert!(writes.len() > before_prompt);
    assert!(String::from_utf8(writes.clone()).expect("utf8 output").contains("during"));

    let after_prompt = writes.len();
    sink.write_to(Output::Frame("after".to_string()), false, &mut writes);
    assert!(writes.len() > after_prompt);
    assert!(String::from_utf8(writes).expect("utf8 output").contains("after"));
}

#[test]
fn prompt_replays_every_append_only_line() {
    let mut sink = Sink::new();
    let mut writes = Vec::new();

    sink.on_prompt_to(PromptAction::Start, &mut writes);
    sink.write_to(Output::Lines(vec!["first".to_string()]), false, &mut writes);
    sink.write_to(Output::Lines(vec!["second".to_string()]), false, &mut writes);
    assert!(writes.is_empty());

    sink.on_prompt_to(PromptAction::End, &mut writes);

    assert_eq!(String::from_utf8(writes).expect("utf8 output"), "first\nsecond\n");
}

#[test]
fn prompt_renders_only_the_latest_buffered_frame() {
    let mut sink = Sink::new();
    let mut writes = Vec::new();

    sink.on_prompt_to(PromptAction::Start, &mut writes);
    sink.write_to(Output::Frame("stale".to_string()), false, &mut writes);
    sink.write_to(Output::Frame("latest".to_string()), false, &mut writes);
    assert!(writes.is_empty());

    sink.on_prompt_to(PromptAction::End, &mut writes);

    let output = String::from_utf8(writes).expect("utf8 output");
    assert!(output.contains("latest"));
    assert!(!output.contains("stale"));
}

/// The cursor-up distances (`\x1b[<n>A`) in `output`.
fn cursor_ups(output: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("\x1b[") {
        rest = &rest[start + 2..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with('A') {
            result.push(digits.parse().expect("parse the cursor-up distance"));
        }
    }
    result
}

/// Regression test for pnpm/pnpm#14270: `pnpm update -g` installs each global
/// package group in turn, so the frame keeps growing by one progress line per
/// group. The differ redraws a line by moving the cursor up from the end of the
/// frame; once the frame is taller than the terminal, the lines it has to reach
/// have scrolled away and the move stops at the top of the screen — landing on,
/// and overwriting, whatever is displayed there instead.
#[test]
fn never_redraws_above_the_top_of_the_terminal() {
    const ROWS: usize = 6;
    const COLUMNS: usize = 120;

    let mut sink = Sink::new();
    sink.terminal_size = || Some((COLUMNS, Some(ROWS)));
    sink.diff = crate::diff::Diff::new(COLUMNS);
    let mut writes = Vec::new();

    // One progress line per group; the first group's line keeps ticking, so
    // redrawing the frame means reaching back to its top.
    let frame = |groups: usize, first_group_resolved: usize| -> String {
        (0..groups)
            .map(|group| {
                let resolved = if group == 0 { first_group_resolved } else { 1 };
                format!("global/install-{group}: Progress: resolved {resolved}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    for groups in 1..=ROWS * 3 {
        sink.write_to(Output::Frame(frame(groups, 1)), false, &mut writes);
    }
    sink.write_to(Output::Frame(frame(ROWS * 3, 2)), false, &mut writes);

    let output = String::from_utf8(writes).expect("utf8 output");
    let ups = cursor_ups(&output);
    assert!(!ups.is_empty(), "the frame must have been redrawn at least once");
    assert!(
        ups.iter().all(|up| *up < ROWS),
        "no redraw may reach above the terminal's top row, got: {ups:?}",
    );
}

/// An error frame stands in for the progress output rather than extending it,
/// so it can be shorter than the prefix already committed to the scrollback.
/// That prefix no longer describes what is on screen, so the replacement is
/// rendered whole instead of being sliced against it.
#[test]
fn a_frame_shorter_than_the_committed_prefix_is_rendered_whole() {
    let mut sink = Sink::new();
    sink.terminal_size = || Some((120, Some(3)));
    sink.diff = crate::diff::Diff::new(120);
    let mut writes = Vec::new();

    let tall = (0..12).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n");
    sink.write_to(Output::Frame(tall), false, &mut writes);
    assert!(sink.committed_lines > 0, "the tall frame must have overflowed the terminal");

    writes.clear();
    sink.write_to(Output::Frame("Error: boom".to_string()), false, &mut writes);

    let output = String::from_utf8(writes).expect("utf8 output");
    assert!(output.contains("Error: boom"), "the error frame must be rendered, got: {output:?}");
}

/// A single logical line can wrap to more rows than the terminal has, and then
/// even the one line the frame must keep has its start off screen. Redrawing it
/// would move the cursor above the top of the screen, so it is reprinted below
/// instead of revised in place.
#[test]
fn a_line_taller_than_the_terminal_is_reprinted_rather_than_revised() {
    const ROWS: usize = 4;
    const COLUMNS: usize = 20;

    let mut sink = Sink::new();
    sink.terminal_size = || Some((COLUMNS, Some(ROWS)));
    sink.diff = crate::diff::Diff::new(COLUMNS);
    let mut writes = Vec::new();

    for resolved in 1..=4 {
        let line = format!(
            "global/install-0: Progress: resolved {resolved}, reused 0, downloaded 0, added 0",
        );
        // `commit_overflow` keeps one row spare for the cursor line.
        assert!(line.len() > COLUMNS * (ROWS - 1), "the line has to outgrow the terminal");
        sink.write_to(Output::Frame(line), false, &mut writes);
    }

    // The frame left over from an unfittable round is just as unreachable, so a
    // shorter frame after one may not be diffed against it either.
    sink.write_to(Output::Frame("Progress: resolved 5".to_string()), false, &mut writes);

    let output = String::from_utf8(writes).expect("utf8 output");
    assert!(output.contains("resolved 4"), "the tall frame must be rendered: {output:?}");
    assert!(output.contains("resolved 5"), "the short frame must be rendered: {output:?}");
    assert!(
        cursor_ups(&output).is_empty(),
        "an unreachable line must not be redrawn, got: {:?}",
        cursor_ups(&output),
    );
}

/// A resize reflows the frame already on screen, so nothing the differ tracked
/// against the old width still describes where anything is. The frame has to be
/// drawn afresh below rather than diffed against a layout that no longer holds.
#[test]
fn a_resize_starts_a_fresh_frame() {
    let frame = || Output::Frame("resolving\nProgress: resolved 1".to_string());

    let mut sink = Sink::new();
    sink.terminal_size = || Some((80, Some(24)));
    sink.diff = crate::diff::Diff::new(80);
    let mut writes = Vec::new();
    sink.write_to(frame(), false, &mut writes);

    sink.terminal_size = || Some((40, Some(24)));
    writes.clear();
    sink.write_to(frame(), false, &mut writes);

    let output = String::from_utf8(writes).expect("utf8 output");
    assert!(
        output.contains("resolving") && output.contains("Progress: resolved 1"),
        "the reflowed frame must be drawn afresh, got: {output:?}",
    );
    assert!(
        cursor_ups(&output).is_empty(),
        "a stale layout must not be moved against, got: {:?}",
        cursor_ups(&output),
    );
}

/// The window can shrink under a frame that fitted when it was drawn. Its top
/// has scrolled away just as surely as an over-tall line's, so it cannot be
/// revised either — including by the handover that commits the overflow.
#[test]
fn a_shrinking_window_starts_a_fresh_frame() {
    const COLUMNS: usize = 80;

    let mut sink = Sink::new();
    sink.terminal_size = || Some((COLUMNS, Some(24)));
    sink.diff = crate::diff::Diff::new(COLUMNS);
    let mut writes = Vec::new();

    let frame = |resolved: usize| -> Output {
        let lines: Vec<String> =
            (0..20).map(|group| format!("install-{group}: resolved {resolved}")).collect();
        Output::Frame(lines.join("\n"))
    };
    sink.write_to(frame(1), false, &mut writes);

    sink.terminal_size = || Some((COLUMNS, Some(6)));
    writes.clear();
    sink.write_to(frame(2), false, &mut writes);

    let output = String::from_utf8(writes).expect("utf8 output");
    assert!(
        cursor_ups(&output).iter().all(|up| *up < 6),
        "a frame the window shrank under must not be moved into, got: {:?}",
        cursor_ups(&output),
    );
}
