use pacquet_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogLevel, ProgressLog, ProgressMessage,
    PromptAction, StatsLog, StatsMessage,
};

use super::{Colors, LogEvent, Output, ReporterState, Sink, is_coalesceable};

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

/// pnpm reports each deprecated package once, on first resolution.
/// pacquet's resolver re-emits a package it later meets at a shallower
/// depth, so the reporter has to fold the repeats away: one line per
/// package, and a package that turns out to be direct must not also be
/// counted among the deprecated subdependencies.
#[test]
fn a_deprecated_package_is_reported_once_even_when_re_emitted() {
    use pacquet_reporter::{DeprecationLog, Stage, StageLog};

    let deprecation = |pkg_id: &str, depth: i32, prefix: &str| {
        LogEvent::Deprecation(DeprecationLog {
            level: LogLevel::Debug,
            pkg_name: "glob".to_string(),
            pkg_version: "10.5.0".to_string(),
            pkg_id: pkg_id.to_string(),
            prefix: prefix.to_string(),
            deprecated: "no longer supported".to_string(),
            depth,
        })
    };

    let mut state = ReporterState::new("/repo".to_string(), 120, Colors { enabled: false }, true);
    let mut rendered = String::new();
    let mut record = |output: Output| {
        if let Output::Lines(lines) = output {
            for line in lines {
                rendered.push_str(&line);
                rendered.push('\n');
            }
        }
    };

    // First met deep in the tree, then again as a direct dependency of
    // two workspace projects.
    record(state.handle(&deprecation("glob@10.5.0", 3, "/repo/packages/a")));
    record(state.handle(&deprecation("glob@10.5.0", 0, "/repo")));
    record(state.handle(&deprecation("glob@10.5.0", 0, "/repo/packages/b")));
    record(state.handle(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: "/repo".to_string(),
        stage: Stage::ResolutionDone,
    })));

    assert_eq!(rendered.matches("deprecated").count(), 1, "rendered:\n{rendered}");
    assert!(
        !rendered.contains("deprecated subdependencies found"),
        "a direct deprecation must not also be summarized as a subdependency:\n{rendered}",
    );
}
