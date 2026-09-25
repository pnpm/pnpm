use super::StreamedScript;
use crate::process_tracker::spawn_child;
use pnpm_reporter::{LifecycleMessage, LifecycleStdio, LogEvent, Reporter};
use pretty_assertions::assert_eq;
use std::{
    fs,
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

/// [pnpm/pnpm#5730](https://github.com/pnpm/pnpm/issues/5730)
#[cfg(unix)]
#[test]
fn pump_stops_reading_output_held_open_by_a_background_process() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .expect("lock")
                .push(event.clone());
        }
    }

    let dir = tempdir().expect("create temp dir");
    let pid_file = dir.path().join("background.pid");
    let mut command = Command::new("sh");
    command
        .args(["-c", r#"echo started; (sleep 2; echo late; exec sleep 30) & echo $! > "$1""#, "sh"])
        .arg(&pid_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_child(&mut command, None).expect("spawn script");
    let streamed = StreamedScript {
        dep_path: "test",
        stage: "prepare",
        wd: "test",
        emit: RecordingReporter::emit,
    };

    let started = Instant::now();
    let status = streamed.pump(&mut child).expect("pump script");
    let elapsed = started.elapsed();
    thread::sleep(Duration::from_secs(3).saturating_sub(started.elapsed()));
    let background = fs::read_to_string(&pid_file).expect("read background pid");
    Command::new("kill")
        .arg(background.trim())
        .status()
        .expect("kill background process");

    assert!(status.success(), "script must exit cleanly: {status:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "pump must not wait for the background process: {elapsed:?}",
    );
    let lines: Vec<_> = EVENTS
        .lock()
        .expect("lock")
        .iter()
        .filter_map(|event| match event {
            LogEvent::Lifecycle(log) => match &log.message {
                LifecycleMessage::Stdio { stdio, line, .. } => Some((*stdio, line.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(lines, [(LifecycleStdio::Stdout, "started".to_string())]);
}
