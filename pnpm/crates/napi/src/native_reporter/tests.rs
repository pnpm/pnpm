use std::sync::{Arc, Mutex};

use pnpm_reporter::{
    LogEvent, LogLevel, ProgressLog, ProgressMessage, StatsLog, StatsMessage, SummaryLog,
};

use super::{Destination, NativeRenderer, ReporterOptions};

const DIR: &str = "/repo";

/// A renderer whose chunks land in memory instead of on a file
/// descriptor, so a test can assert on what the host would have received.
fn renderer(options: &ReporterOptions) -> (NativeRenderer, Arc<Mutex<String>>) {
    let buffer = Arc::new(Mutex::new(String::new()));
    let renderer =
        NativeRenderer::with_destination(options, DIR, Destination::Buffer(Arc::clone(&buffer)));
    (renderer, buffer)
}

fn written(buffer: &Arc<Mutex<String>>) -> String {
    buffer.lock().unwrap().clone()
}

fn progress() -> LogEvent {
    LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Resolved {
            package_id: "registry.npmjs.org/foo/1.0.0".to_string(),
            requester: DIR.to_string(),
        },
    })
}

fn stats(added: u64) -> LogEvent {
    LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: DIR.to_string(), added },
    })
}

fn summary() -> LogEvent {
    LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix: DIR.to_string() })
}

#[test]
fn append_only_writes_each_update_as_a_line() {
    let (mut renderer, buffer) =
        renderer(&ReporterOptions { append_only: Some(true), ..ReporterOptions::default() });

    renderer.handle(&progress());

    let output = written(&buffer);
    assert!(output.contains("Progress: resolved 1"), "output: {output:?}");
    assert!(output.ends_with('\n'), "output: {output:?}");
}

/// In-place mode wraps each redraw in the cursor-control sequences the
/// frame differ needs; a host writing them to a terminal gets pnpm's
/// live-updating progress line.
#[test]
fn in_place_mode_writes_a_frame_with_cursor_control() {
    let (mut renderer, buffer) =
        renderer(&ReporterOptions { append_only: Some(false), ..ReporterOptions::default() });

    renderer.handle(&progress());

    let output = written(&buffer);
    assert!(output.starts_with('\r'), "output: {output:?}");
    assert!(output.ends_with("\x1b[K\x1b[0J"), "output: {output:?}");
}

/// Progress redraws inside the throttle window are dropped, but the state
/// still folds them — the next event that is not a progress update renders
/// the current counts.
#[test]
fn progress_is_throttled_and_the_next_non_progress_event_catches_up() {
    let (mut renderer, buffer) = renderer(&ReporterOptions {
        append_only: Some(true),
        throttle_progress: Some(60_000),
        ..ReporterOptions::default()
    });

    renderer.handle(&progress());
    let after_first = written(&buffer);
    renderer.handle(&progress());
    assert_eq!(written(&buffer), after_first, "the second redraw is inside the throttle window");

    renderer.handle(&stats(2));
    renderer.handle(&summary());
    let after_summary = written(&buffer);
    assert!(after_summary.len() > after_first.len(), "output: {after_summary:?}");
    assert!(after_summary.contains("Packages: +2"), "output: {after_summary:?}");
}

/// A zero throttle turns coalescing off, which is what a test or a
/// non-interactive log capture wants.
#[test]
fn a_zero_throttle_renders_every_progress_update() {
    let (mut renderer, buffer) = renderer(&ReporterOptions {
        append_only: Some(true),
        throttle_progress: Some(0),
        ..ReporterOptions::default()
    });

    renderer.handle(&progress());
    let after_first = written(&buffer);
    renderer.handle(&progress());

    assert!(written(&buffer).len() > after_first.len());
}

#[test]
fn color_is_off_by_default_for_a_callback_destination() {
    let (mut renderer, buffer) =
        renderer(&ReporterOptions { append_only: Some(true), ..ReporterOptions::default() });

    renderer.handle(&progress());

    assert!(!written(&buffer).contains("\x1b["), "output: {:?}", written(&buffer));
}

#[test]
fn color_can_be_turned_on_explicitly() {
    let (mut renderer, buffer) = renderer(&ReporterOptions {
        append_only: Some(true),
        color: Some(true),
        ..ReporterOptions::default()
    });

    renderer.handle(&progress());

    assert!(written(&buffer).contains("\x1b["), "output: {:?}", written(&buffer));
}

/// The reporting options reach the folded reporter state, not just the
/// renderer that writes its output.
#[test]
fn the_summary_omits_linked_packages_matching_the_hide_pattern() {
    let (mut renderer, buffer) = renderer(&ReporterOptions {
        append_only: Some(true),
        hide_linked_pkgs_diff: Some(vec!["@acme/*".to_string()]),
        ..ReporterOptions::default()
    });

    renderer.handle(&linked_root("@acme/runtime"));
    renderer.handle(&linked_root("@other/tool"));
    renderer.handle(&summary());

    let output = written(&buffer);
    assert!(!output.contains("@acme/runtime"), "output: {output}");
    assert!(output.contains("@other/tool"), "output: {output}");
}

fn linked_root(name: &str) -> LogEvent {
    LogEvent::Root(pnpm_reporter::RootLog {
        level: LogLevel::Debug,
        message: pnpm_reporter::RootMessage::Added {
            prefix: DIR.to_string(),
            added: pnpm_reporter::AddedRoot {
                name: name.to_string(),
                real_name: name.to_string(),
                version: None,
                dependency_type: Some(pnpm_reporter::DependencyType::Prod),
                id: None,
                latest: None,
                linked_from: Some("/elsewhere".to_string()),
            },
        },
    })
}

#[test]
fn an_unknown_log_level_falls_back_to_info() {
    assert_eq!(
        format!("{:?}", super::parse_log_level(Some("shout"))),
        format!("{:?}", super::parse_log_level(None)),
    );
}

/// A host that computed its width the way pnpm does — the terminal's
/// columns less two — can arrive at zero from a one- or two-column
/// terminal. The renderer floors it rather than trying to wrap at nothing.
#[test]
fn a_zero_width_is_floored_to_one_column() {
    let (mut renderer, buffer) = renderer(&ReporterOptions {
        append_only: Some(true),
        width: Some(0),
        ..ReporterOptions::default()
    });

    renderer.handle(&progress());

    assert!(!written(&buffer).is_empty());
}
