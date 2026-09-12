use super::{
    CWD, DependencyType, ExecutionTimeLog, LogEvent, LogLevel, ReporterOptions, StatsLog,
    StatsMessage, added_root, deprecation, emitted_lines, importing_done,
    interleaved_lifecycle_events, pretty_bytes, progress, render, scope, scope_reporting_state,
    state, state_with_options, summary,
};

#[test]
fn pretty_bytes_truncates_without_floating_point_drift() {
    assert_eq!(pretty_bytes(1_130), "1.13 kB");
    assert_eq!(pretty_bytes(1_130_000), "1.13 MB");
}

#[test]
fn importing_done_appends_done_suffix() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![progress("resolved"), progress("imported"), importing_done()]);
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 1, done");
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
        "Packages: +1\n+\n\ndependencies:\n+ foo 1.0.0\n\n\
         Progress: resolved 1, reused 1, downloaded 0, added 1, done\n\
         Done in 1.2s using pnpm v0.0.1",
    );
}

#[test]
fn recursive_direct_deprecation_is_zoomed_and_omits_the_message() {
    let mut reporter =
        state_with_options(ReporterOptions { is_recursive: true, ..ReporterOptions::default() });
    let frame = render(&mut reporter, vec![deprecation("express", "0.14.1", 0, CWD)]);
    assert_eq!(
        frame,
        pnpm_default_reporter::format::zoom_out(CWD, CWD, "[WARN] deprecated express@0.14.1",),
    );
}

/// Upstream's zoomed variant carries only `deprecated name@version` — the
/// deprecation text is dropped.
#[test]
fn zoomed_direct_deprecation_omits_the_message() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![deprecation("express", "0.14.1", 0, "/repo/packages/app")]);
    assert_eq!(
        frame,
        pnpm_default_reporter::format::zoom_out(
            CWD,
            "/repo/packages/app",
            "[WARN] deprecated express@0.14.1",
        ),
    );
}

/// A single selected project is the directory the user is standing in, so
/// pnpm says nothing — even for a command that reports scope.
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
        aggregate_output: true,
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
