use super::{
    CWD,
    DependencyType,
    ExecutionTimeLog,
    LogEvent,
    LogLevel,
    ReporterOptions,
    StatsLog,
    StatsMessage,
    added_root,
    emitted_lines,
    importing_done,
    interleaved_lifecycle_events,
    pretty_bytes,
    progress,
    render,
    scope,
    scope_reporting_state,
    state,
    state_with_options,
    summary,
};

#[test]
fn pretty_bytes_truncates_without_floating_point_drift() {
    assert_eq!(pretty_bytes(1_130), "1.13 kB");
    assert_eq!(pretty_bytes(1_130_000), "1.13 MB");
}

#[test]
fn the_live_packages_line_waits_for_the_stats() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![progress("resolved"), progress("imported"), importing_done()]);
    assert_eq!(frame, "Packages: +1 of 1 resolved");
}

#[test]
fn stats_bar_is_colored_when_enabled() {
    let mut reporter = state(true);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added { prefix: CWD.to_string(), added: 1 },
            }),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Removed { prefix: CWD.to_string(), removed: 0 },
            }),
        ],
    );
    assert_eq!(frame, "Packages: \u{1b}[32m+1\u{1b}[39m\n\u{1b}[32m+\u{1b}[39m");
}

#[test]
fn full_install_frame_orders_blocks_like_pnpm() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            progress("resolved"),
            progress("found_in_store"),
            progress("imported"),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added { prefix: CWD.to_string(), added: 1 },
            }),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Removed { prefix: CWD.to_string(), removed: 0 },
            }),
            added_root("foo", "1.0.0", DependencyType::Prod),
            summary(),
            importing_done(),
            LogEvent::ExecutionTime(ExecutionTimeLog {
                level: LogLevel::Debug,
                started_at: 0,
                ended_at: 1200,
            }),
        ],
    );
    assert_eq!(
        frame,
        "Packages: +1 (100% reused)\n+\n\ndependencies:\n+ foo 1.0.0\n\nDone in 1.2s using pnpm v0.0.1",
    );
}

/// A single selected project is the directory the user is standing in, so
/// pnpm says nothing, even for a command that reports scope.
#[test]
fn stays_silent_for_a_single_selected_project() {
    let mut reporter = scope_reporting_state();
    assert!(render(&mut reporter, vec![scope(1, Some(3), Some(CWD))]).is_empty());
}

/// The pattern hides *linked* instances only. The same package really
/// installed from the registry is a change the summary must still report.
#[test]
fn a_hide_linked_pattern_keeps_the_same_package_when_it_is_installed() {
    let mut reporter = state_with_options(ReporterOptions {
        hide_linked_pkgs_diff: vec!["@acme/*".to_string()],
        ..ReporterOptions::default()
    });

    let frame = render(
        &mut reporter,
        vec![added_root("@acme/runtime", "1.0.0", DependencyType::Prod), summary()],
    );

    assert!(frame.contains("@acme/runtime"), "frame: {frame}");
}

/// Port of upstream's `groups lifecycle output when append-only and
/// aggregate-output are used with mixed stages`: each script's lines are
/// withheld until it exits, so an interleaving sibling cannot split them.
#[test]
fn aggregate_output_withholds_each_script_until_it_exits() {
    let mut reporter = state_with_options(ReporterOptions {
        append_only: true,
        lifecycle: pnpm_default_reporter::state::LifecycleOptions {
            aggregate_output: true,
            ..Default::default()
        },
        ..ReporterOptions::default()
    });

    let lines = emitted_lines(&mut reporter, interleaved_lifecycle_events());

    assert_eq!(
        lines,
        [
            "packages/bar postinstall$ node bar\npackages/bar postinstall: bar I\npackages/bar postinstall: Done",
            "packages/foo postinstall$ node foo\npackages/foo postinstall: foo I\npackages/foo postinstall: foo II\npackages/foo postinstall: Done",
        ],
    );
}

#[test]
fn stats_bar_colors_downloaded_and_reused_packages() {
    let mut reporter = state(true);
    let frame = render(
        &mut reporter,
        vec![
            progress("fetched"),
            progress("found_in_store"),
            progress("found_in_store"),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added { prefix: CWD.to_string(), added: 3 },
            }),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Removed { prefix: CWD.to_string(), removed: 0 },
            }),
            importing_done(),
        ],
    );
    let mut lines = frame.lines();
    assert_eq!(lines.next(), Some("Packages: \u{1b}[32m+3\u{1b}[39m (66% reused)"));
    let bar = lines.next().expect("the bar follows the stats line");
    let downloaded = "\u{1b}[32m↓\u{1b}[39m";
    let reused = "\u{1b}[32m+\u{1b}[39m";
    assert_eq!(bar, format!("{downloaded}{reused}{reused}"));
}

/// Arrived packages wait in the default color, and turn green once added.
#[test]
fn live_bar_turns_green_as_packages_are_added() {
    let mut reporter = state(true);
    let frame = render(
        &mut reporter,
        vec![progress("fetched"), progress("found_in_store"), progress("imported")],
    );
    let bar = frame
        .lines()
        .nth(1)
        .expect("the bar follows the live line");
    assert_eq!(bar, "\u{1b}[32m↓\u{1b}[39m+");
}
