use pacquet_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogLevel, ProgressLog, ProgressMessage, StatsLog,
    StatsMessage,
};

use super::{LogEvent, is_coalesceable};

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
