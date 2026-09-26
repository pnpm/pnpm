use super::{
    CWD, LifecycleLog, LifecycleMessage, LifecycleStdio, LockfileVerificationLog,
    LockfileVerificationMessage, LogEvent, LogLevel, MaxLogLevel, Output, ReporterOptions,
    emitted_lines, ignored_scripts, lifecycle_exit, lifecycle_line, lifecycle_script,
    state_with_options,
};

fn assert_hidden(output: Output) {
    match output {
        Output::None => {}
        Output::Frame(frame) => assert_eq!(frame, ""),
        Output::Lines(lines) => assert!(lines.is_empty(), "{lines:?}"),
    }
}

#[test]
fn quiet_failure_retains_all_output_in_append_only_and_tty_renderers() {
    for (max_log_level, append_only) in [
        (MaxLogLevel::Warn, false),
        (MaxLogLevel::Warn, true),
        (MaxLogLevel::Error, false),
        (MaxLogLevel::Error, true),
    ] {
        let mut reporter = state_with_options(ReporterOptions {
            max_log_level,
            append_only,
            ..Default::default()
        });
        let script = lifecycle_script(CWD, "postinstall", "node build.js");
        assert_hidden(reporter.handle(&script));
        for index in 0..28 {
            let event = LogEvent::Lifecycle(LifecycleLog {
                level: LogLevel::Debug,
                message: LifecycleMessage::Stdio {
                    dep_path: CWD.to_string(),
                    wd: CWD.to_string(),
                    stage: "postinstall".to_string(),
                    line: format!("diagnostic-{index}-{}", "x".repeat(100)),
                    stdio: [LifecycleStdio::Stdout, LifecycleStdio::Stderr][index % 2],
                },
            });
            assert_hidden(reporter.handle(&event));
        }
        let output = reporter.handle(&lifecycle_exit(CWD, "postinstall", 1));
        let text = match output {
            Output::Frame(text) => text,
            Output::Lines(lines) => lines.join("\n"),
            Output::None => panic!("failure must flush its diagnostics"),
        };
        eprintln!("{max_log_level:?}, append_only={append_only}:\n{text}");
        assert!(text.contains("postinstall$ node build.js"));
        for index in 0..28 {
            assert!(text.contains(&format!("diagnostic-{index}-{}", "x".repeat(100))));
        }
        assert!(text.contains("Failed"));
        assert!(!text.contains("collapsed"));
    }
}

#[test]
fn optional_failures_are_warn_only_and_completed_buffers_never_leak() {
    for max_log_level in [MaxLogLevel::Warn, MaxLogLevel::Error] {
        let mut reporter = state_with_options(ReporterOptions {
            max_log_level,
            append_only: true,
            ..Default::default()
        });
        let mut optional_exit = lifecycle_exit(CWD, "postinstall", 1);
        if let LogEvent::Lifecycle(LifecycleLog {
            message: LifecycleMessage::Exit { optional, .. },
            ..
        }) = &mut optional_exit
        {
            *optional = true;
        }
        let lines = emitted_lines(
            &mut reporter,
            vec![
                lifecycle_script(CWD, "postinstall", "node optional.js"),
                lifecycle_line(CWD, "postinstall", "optional-output"),
                optional_exit,
            ],
        );
        if max_log_level == MaxLogLevel::Warn {
            assert!(lines.join("\n").contains("optional-output"));
            assert!(lines.join("\n").contains("skipped as optional"));
        } else {
            assert!(lines.is_empty());
        }
        let lines = emitted_lines(
            &mut reporter,
            vec![
                lifecycle_script(CWD, "postinstall", "node success.js"),
                lifecycle_line(CWD, "postinstall", "successful-output"),
                lifecycle_exit(CWD, "postinstall", 0),
                lifecycle_script(CWD, "postinstall", "node required.js"),
                lifecycle_line(CWD, "postinstall", "required-output"),
                lifecycle_exit(CWD, "postinstall", 1),
            ],
        );
        let text = lines.join("\n");
        assert!(text.contains("required-output"));
        assert!(!text.contains("optional-output"));
        assert!(!text.contains("successful-output"));
    }
}

#[test]
fn interleaved_package_and_stage_buffers_flush_independently() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Warn,
        append_only: true,
        ..Default::default()
    });
    let lines = emitted_lines(
        &mut reporter,
        vec![
            lifecycle_script(CWD, "preinstall", "node before.js"),
            lifecycle_script(CWD, "postinstall", "node after.js"),
            lifecycle_script("/repo/other", "preinstall", "node other.js"),
            lifecycle_line(CWD, "preinstall", "before-output"),
            lifecycle_line(CWD, "postinstall", "after-output"),
            lifecycle_line("/repo/other", "preinstall", "other-output"),
            lifecycle_exit(CWD, "postinstall", 1),
            lifecycle_exit("/repo/other", "preinstall", 0),
            lifecycle_exit(CWD, "preinstall", 1),
        ],
    );
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("after-output"));
    assert!(!lines[0].contains("before-output"));
    assert!(lines[1].contains("before-output"));
    assert!(!lines.join("\n").contains("other-output"));
}

fn with_dep_path(mut event: LogEvent, dep_path: &str) -> LogEvent {
    let LogEvent::Lifecycle(LifecycleLog { message, .. }) = &mut event else { unreachable!() };
    match message {
        LifecycleMessage::Script { dep_path: target, .. }
        | LifecycleMessage::Stdio { dep_path: target, .. }
        | LifecycleMessage::Exit { dep_path: target, .. } => *target = dep_path.to_string(),
    }
    event
}

#[test]
fn same_named_projects_buffer_separately() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Warn,
        append_only: true,
        ..Default::default()
    });
    let lines = emitted_lines(
        &mut reporter,
        [
            lifecycle_script("/repo/a", "(exec)", "node fail.js"),
            lifecycle_line("/repo/a", "(exec)", "failed-output"),
            lifecycle_script("/repo/b", "(exec)", "node ok.js"),
            lifecycle_line("/repo/b", "(exec)", "successful-output"),
            lifecycle_exit("/repo/b", "(exec)", 0),
            lifecycle_exit("/repo/a", "(exec)", 1),
        ]
        .into_iter()
        .map(|event| with_dep_path(event, "same-name"))
        .collect(),
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("node fail.js"));
    assert!(lines[0].contains("failed-output"));
    assert!(!lines[0].contains("successful-output"));
}

#[test]
fn info_aggregate_retains_success_and_optional_failure_blocks() {
    let mut reporter = state_with_options(ReporterOptions {
        append_only: true,
        lifecycle: pnpm_default_reporter::state::LifecycleOptions {
            aggregate_output: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let mut optional_exit = lifecycle_exit(CWD, "postinstall", 1);
    if let LogEvent::Lifecycle(LifecycleLog {
        message: LifecycleMessage::Exit { optional, .. },
        ..
    }) = &mut optional_exit
    {
        *optional = true;
    }
    let lines = emitted_lines(
        &mut reporter,
        vec![
            lifecycle_script(CWD, "preinstall", "node before.js"),
            lifecycle_line(CWD, "preinstall", "successful-output"),
            lifecycle_exit(CWD, "preinstall", 0),
            lifecycle_script(CWD, "postinstall", "node after.js"),
            lifecycle_line(CWD, "postinstall", "optional-output"),
            optional_exit,
        ],
    );
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("successful-output"));
    assert!(lines[0].contains("Done"));
    assert!(lines[1].contains("optional-output"));
    assert!(lines[1].contains("Failed"));
}

#[test]
fn quiet_verification_prints_only_failures() {
    for max_log_level in [MaxLogLevel::Warn, MaxLogLevel::Error] {
        let mut reporter = state_with_options(ReporterOptions {
            max_log_level,
            append_only: true,
            ..Default::default()
        });
        let events = [
            LockfileVerificationMessage::Started { entries: 2, lockfile_path: None },
            LockfileVerificationMessage::Cached { verified_at: None, lockfile_path: None },
            LockfileVerificationMessage::Done { entries: 2, elapsed_ms: 100, lockfile_path: None },
            LockfileVerificationMessage::Failed {
                entries: 2,
                elapsed_ms: 100,
                lockfile_path: None,
            },
        ];
        for (index, message) in events.into_iter().enumerate() {
            let output =
                reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
                    level: LogLevel::Debug,
                    message,
                }));
            assert_eq!(matches!(output, Output::Lines(_)), index == 3);
        }
        let lines = emitted_lines(&mut reporter, vec![ignored_scripts(&["esbuild"])]);
        assert_eq!(!lines.is_empty(), max_log_level == MaxLogLevel::Warn);
    }
}
