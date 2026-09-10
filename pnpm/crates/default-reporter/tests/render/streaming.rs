use super::{
    CWD, ExecutionTimeLog, LogEvent, LogLevel, MaxLogLevel, Output, ReporterOptions, Stage,
    fetching_in_progress, fetching_started, interleaved_lifecycle_events, lifecycle_stdio_events,
    pnpm_log, progress, progress_at, render, stage_at, state, state_with_options,
};

#[test]
fn prints_progress_on_first_download() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![stage_at(CWD, Stage::ResolutionStarted), progress("resolved"), progress("fetched")],
    );
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 1, added 0");
}

#[test]
fn prints_progress_of_big_files_download() {
    const MIB: u64 = 1024 * 1024;
    let pkg_1 = "registry.npmjs.org/foo/1.0.0";
    let pkg_3 = "registry.npmjs.org/qar/3.0.0";
    let mut reporter = state(false);
    let events = vec![
        stage_at(CWD, Stage::ResolutionStarted),
        progress("resolved"),
        fetching_started(pkg_1, 10 * MIB, 1),
        fetching_in_progress(pkg_1, 11 * MIB / 2),
        progress_at(CWD, "resolved"),
        fetching_started(pkg_1, 10, 1),
        fetching_in_progress(pkg_1, 7 * MIB),
        progress_at(CWD, "resolved"),
        fetching_started(pkg_3, 20 * MIB, 1),
        fetching_in_progress(pkg_3, 19 * MIB),
        fetching_in_progress(pkg_1, 10 * MIB),
    ];
    let mut frames = Vec::new();
    for event in events {
        if let Output::Frame(frame) = reporter.handle(&event)
            && !frame.is_empty()
        {
            frames.push(frame);
        }
    }

    assert_eq!(
        frames,
        vec![
            "Progress: resolved 1, reused 0, downloaded 0, added 0".to_string(),
            format!(
                "Progress: resolved 1, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 0.00 B/10.48 MB",
            ),
            format!(
                "Progress: resolved 1, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 5.76 MB/10.48 MB",
            ),
            format!(
                "Progress: resolved 2, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 5.76 MB/10.48 MB",
            ),
            format!(
                "Progress: resolved 2, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 7.34 MB/10.48 MB",
            ),
            format!(
                "Progress: resolved 3, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 7.34 MB/10.48 MB",
            ),
            format!(
                "Progress: resolved 3, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 7.34 MB/10.48 MB\n\
                 Downloading {pkg_3}: 0.00 B/20.97 MB",
            ),
            format!(
                "Progress: resolved 3, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_1}: 7.34 MB/10.48 MB\n\
                 Downloading {pkg_3}: 19.92 MB/20.97 MB",
            ),
            format!(
                "Downloading {pkg_1}: 10.48 MB/10.48 MB, done\n\
                 Progress: resolved 3, reused 0, downloaded 0, added 0\n\
                 Downloading {pkg_3}: 19.92 MB/20.97 MB",
            ),
        ],
    );
}

#[test]
fn loglevel_error_suppresses_warnings_and_the_visual_streams() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Error,
        ..ReporterOptions::default()
    });
    let frame = render(
        &mut reporter,
        vec![
            pnpm_log(LogLevel::Warn, "deprecated package"),
            pnpm_log(LogLevel::Info, "Already up to date"),
            progress("resolved"),
            LogEvent::ExecutionTime(ExecutionTimeLog {
                level: LogLevel::Debug,
                started_at: 0,
                ended_at: 1200,
            }),
        ],
    );
    assert_eq!(frame, "");
}

#[test]
fn append_only_streams_each_lifecycle_output_line() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });

    let mut lines = Vec::new();
    for event in lifecycle_stdio_events() {
        if let Output::Lines(emitted) = reporter.handle(&event) {
            lines.extend(emitted);
        }
    }

    assert!(lines.iter().any(|line| line.contains("downloading the binary")), "lines: {lines:#?}");
}

/// `hideLifecycleOutput` keeps the script's output in its collapsed block
/// rather than streaming it, even under append-only rendering — pnpm's
/// behavior for an embedder that owns the surrounding terminal output.
#[test]
fn hide_lifecycle_output_stops_the_streaming_even_under_append_only() {
    let mut reporter = state_with_options(ReporterOptions {
        append_only: true,
        hide_lifecycle_output: true,
        ..ReporterOptions::default()
    });

    let mut lines = Vec::new();
    for event in lifecycle_stdio_events() {
        if let Output::Lines(emitted) = reporter.handle(&event) {
            lines.extend(emitted);
        }
    }

    assert!(!lines.iter().any(|line| line.contains("downloading the binary")), "lines: {lines:#?}");
}

/// Port of upstream's `groups lifecycle output when streamLifecycleOutput
/// is used` (`cli/default-reporter/test/reportingLifecycleScripts.ts`):
/// `--stream` streams the lifecycle lines even though the rest of the
/// frame still renders in place.
#[test]
fn stream_lifecycle_output_streams_without_append_only() {
    let mut reporter = state_with_options(ReporterOptions {
        stream_lifecycle_output: true,
        ..ReporterOptions::default()
    });

    let frame = render(&mut reporter, interleaved_lifecycle_events());

    assert_eq!(
        frame,
        "\
packages/foo postinstall$ node foo
packages/foo postinstall: foo I
packages/bar postinstall$ node bar
packages/bar postinstall: bar I
packages/foo postinstall: foo II
packages/bar postinstall: Done
packages/foo postinstall: Done",
    );
}
