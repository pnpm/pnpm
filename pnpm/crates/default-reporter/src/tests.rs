use pacquet_reporter::{
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
fn stats_renders_immediately() {
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
