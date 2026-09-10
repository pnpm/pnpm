use super::{
    Envelope, FetchingProgressLog, FetchingProgressMessage, LifecycleLog, LifecycleMessage,
    LifecycleStdio, LogEvent, LogLevel, Mutex, PackageImportMethod, Pipe, ProgressLog,
    ProgressMessage, Reporter, SilentReporter, Stage, StageLog, SummaryLog, Value, assert_eq,
};

/// Summary log serializes with the channel name flattened into the
/// envelope alongside `prefix` and the [bunyan]-envelope `level`.
/// `prefix` is what pnpm's reporter uses to find the matching
/// `pnpm:root` history and render its "+N -M" block.
///
/// [bunyan]: https://github.com/trentm/node-bunyan
#[test]
fn summary_event_matches_pnpm_wire_shape() {
    let event = LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: "/some/project".to_string(),
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:summary");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["prefix"], "/some/project");
}

/// `pnpm:progress` flattens its `status`-tagged payload into the
/// envelope. The three "store-ish" statuses (`resolved`, `fetched`,
/// `found_in_store`) carry `packageId` and `requester`; `imported`
/// substitutes `method` / `to` with no `packageId`. Mirroring pnpm's
/// shape exactly because the JS reporter's switch on `status` is the
/// dispatch.
#[test]
fn progress_event_matches_pnpm_wire_shape() {
    for (message, expected_status) in [
        (
            ProgressMessage::Resolved {
                package_id: "react@18.0.0".to_string(),
                requester: "/proj".to_string(),
            },
            "resolved",
        ),
        (
            ProgressMessage::Fetched {
                package_id: "react@18.0.0".to_string(),
                requester: "/proj".to_string(),
            },
            "fetched",
        ),
        (
            ProgressMessage::FoundInStore {
                package_id: "react@18.0.0".to_string(),
                requester: "/proj".to_string(),
            },
            "found_in_store",
        ),
    ] {
        let event = LogEvent::Progress(ProgressLog { level: LogLevel::Debug, message });
        let envelope =
            Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };

        let json: Value = envelope
            .pipe_ref(serde_json::to_string)
            .expect("serialize envelope")
            .pipe_as_ref(serde_json::from_str)
            .expect("parse JSON");

        assert_eq!(json["name"], "pnpm:progress");
        assert_eq!(json["level"], "debug");
        assert_eq!(json["status"], expected_status);
        assert_eq!(json["packageId"], "react@18.0.0");
        assert_eq!(json["requester"], "/proj");
    }

    let event = LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Imported {
            method: PackageImportMethod::Hardlink,
            requester: "/proj".to_string(),
            to: "/proj/node_modules/.pacquet/react@18.0.0/node_modules/react".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");

    assert_eq!(json["name"], "pnpm:progress");
    assert_eq!(json["status"], "imported");
    assert_eq!(json["method"], "hardlink");
    assert_eq!(json["requester"], "/proj");
    assert_eq!(json["to"], "/proj/node_modules/.pacquet/react@18.0.0/node_modules/react");
    // `imported` deliberately omits `packageId` — match pnpm's shape
    // so consumers that read `progress.packageId` only on the three
    // store-ish statuses don't trip on a stray field.
    assert!(json.get("packageId").is_none(), "imported must not carry packageId");
}

/// `pnpm:fetching-progress` flattens its two-state `status` enum into
/// the envelope. `started` carries `attempt` / `packageId` / `size`
/// (the `Content-Length`-derived value, serialized as JSON `null`
/// when the response is chunked / unknown); `in_progress` carries the
/// running `downloaded` byte count.
#[test]
fn fetching_progress_event_matches_pnpm_wire_shape() {
    let event = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: 1,
            package_id: "react@18.0.0".to_string(),
            size: Some(123_456),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["name"], "pnpm:fetching-progress");
    assert_eq!(json["status"], "started");
    assert_eq!(json["attempt"], 1);
    assert_eq!(json["packageId"], "react@18.0.0");
    assert_eq!(json["size"], 123_456);

    // Unknown / chunked response: `size` must serialize as JSON null,
    // matching pnpm's `size: number | null` shape. The default-reporter
    // checks `size != null` to decide whether to render a percent
    // gauge; emitting an absent field would silently break that.
    let event = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: 1,
            package_id: "react@18.0.0".to_string(),
            size: None,
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert!(json.get("size").is_some_and(serde_json::Value::is_null), "size must be JSON null");

    let event = LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::InProgress {
            downloaded: 65_536,
            package_id: "react@18.0.0".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    assert_eq!(json["status"], "in_progress");
    assert_eq!(json["downloaded"], 65_536);
    assert_eq!(json["packageId"], "react@18.0.0");
}

/// `pnpm:lifecycle` is presence-tagged on `script` / `line` / `exitCode`.
/// pnpm's reporter dispatches on which of those is present rather than
/// on a `status` discriminator. The shared fields (`depPath`, `stage`,
/// `wd`) appear on every record. Field names use camelCase
/// (`depPath`, `exitCode`) so `@pnpm/cli.default-reporter` parses them.
#[test]
fn lifecycle_event_matches_pnpm_wire_shape() {
    eprintln!("CASE: Script");
    let event = LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Script {
            dep_path: "/x@1.0.0".to_string(),
            optional: false,
            script: "node build.js".to_string(),
            stage: "postinstall".to_string(),
            wd: "/proj/node_modules/.pacquet/x@1.0.0/node_modules/x".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["name"], "pnpm:lifecycle");
    assert_eq!(json["level"], "debug");
    assert_eq!(json["depPath"], "/x@1.0.0");
    assert_eq!(json["optional"], false);
    assert_eq!(json["script"], "node build.js");
    assert_eq!(json["stage"], "postinstall");
    assert_eq!(json["wd"], "/proj/node_modules/.pacquet/x@1.0.0/node_modules/x");
    for k in ["line", "stdio", "exitCode"] {
        assert!(json.get(k).is_none(), "Script must not carry {k}, got {json:?}");
    }

    eprintln!("CASE: Stdio");
    let event = LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Stdio {
            dep_path: "/x@1.0.0".to_string(),
            line: "hello world".to_string(),
            stage: "postinstall".to_string(),
            stdio: LifecycleStdio::Stdout,
            wd: "/wd".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["depPath"], "/x@1.0.0");
    assert_eq!(json["line"], "hello world");
    assert_eq!(json["stdio"], "stdout");
    assert_eq!(json["stage"], "postinstall");
    assert_eq!(json["wd"], "/wd");
    for k in ["script", "exitCode", "optional"] {
        assert!(json.get(k).is_none(), "Stdio must not carry {k}, got {json:?}");
    }

    eprintln!("CASE: Exit");
    let event = LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Exit {
            dep_path: "/x@1.0.0".to_string(),
            exit_code: 0,
            optional: false,
            stage: "postinstall".to_string(),
            wd: "/wd".to_string(),
        },
    });
    let envelope = Envelope { time: 1_700_000_000_000, hostname: "host", pid: 4242, event: &event };
    let json: Value = envelope
        .pipe_ref(serde_json::to_string)
        .expect("serialize envelope")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse JSON");
    dbg!(&json);
    assert_eq!(json["depPath"], "/x@1.0.0");
    assert_eq!(json["exitCode"], 0);
    assert_eq!(json["optional"], false);
    assert_eq!(json["stage"], "postinstall");
    assert_eq!(json["wd"], "/wd");
    for k in ["script", "line", "stdio"] {
        assert!(json.get(k).is_none(), "Exit must not carry {k}, got {json:?}");
    }
}

/// [`SilentReporter`] is observably a no-op. Any test fake is harder
/// to write than just calling it.
#[test]
fn silent_reporter_drops_events() {
    SilentReporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: String::new(),
        stage: Stage::ImportingStarted,
    }));
}

#[test]
fn recording_fake_captures_emitted_events() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    fn install_step<Reporter: self::Reporter>() {
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: "/proj".to_string(),
            stage: Stage::ImportingStarted,
        }));
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: "/proj".to_string(),
            stage: Stage::ImportingDone,
        }));
    }

    install_step::<RecordingReporter>();

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(matches!(
        &captured[0],
        LogEvent::Stage(StageLog { stage: Stage::ImportingStarted, .. })
    ));
    assert!(matches!(&captured[1], LogEvent::Stage(StageLog { stage: Stage::ImportingDone, .. })));
}
