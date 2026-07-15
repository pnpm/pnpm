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

    sink.on_prompt(PromptAction::Start);
    sink.write_to(Output::Frame("during".to_string()), false, &mut writes);
    assert_eq!(writes.len(), before_prompt);

    sink.on_prompt(PromptAction::End);
    sink.write_to(Output::Frame("after".to_string()), false, &mut writes);
    assert!(writes.len() > before_prompt);
    assert!(String::from_utf8(writes).expect("utf8 output").contains("after"));
}
