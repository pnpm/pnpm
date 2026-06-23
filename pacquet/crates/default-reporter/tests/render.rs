//! Frame-level tests: drive sequences of `LogEvent`s through `ReporterState`
//! and assert the rendered output, matching what `@pnpm/cli.default-reporter`
//! produces for the same events. Colors are constructed off for readable
//! plain-text assertions and on for the ANSI-specific ones.

use chrono::{DateTime, Utc};
use pacquet_default_reporter::{
    colors::Colors,
    state::{Output, ReporterState},
};
use pacquet_reporter::{
    AddedRoot, BrokenModulesLog, ContextLog, DependencyType, ExecutionTimeLog, HookLog,
    LifecycleLog, LifecycleMessage, LifecycleStdio, LockfileVerificationLog,
    LockfileVerificationMessage, LogEvent, LogLevel, PackageImportMethod, PackageImportMethodLog,
    PnpmLog, ProgressLog, ProgressMessage, RootLog, RootMessage, Stage, StageLog, StatsLog,
    StatsMessage, SummaryLog,
};

const CWD: &str = "/repo";

fn state(colors: bool) -> ReporterState {
    ReporterState::new(CWD.to_string(), 80, Colors { enabled: colors }, false)
}

/// Feed events through the in-place renderer and return the last full frame.
fn render(state: &mut ReporterState, events: Vec<LogEvent>) -> String {
    let mut last = String::new();
    for event in events {
        if let Output::Frame(frame) = state.handle(&event) {
            last = frame;
        }
    }
    last
}

fn progress(status: &str) -> LogEvent {
    let requester = CWD.to_string();
    let package_id = "registry.npmjs.org/foo/1.0.0".to_string();
    let message = match status {
        "resolved" => ProgressMessage::Resolved { package_id, requester },
        "fetched" => ProgressMessage::Fetched { package_id, requester },
        "found_in_store" => ProgressMessage::FoundInStore { package_id, requester },
        "imported" => ProgressMessage::Imported {
            method: PackageImportMethod::Hardlink,
            requester,
            to: "/repo/node_modules/foo".to_string(),
        },
        other => panic!("unknown status {other}"),
    };
    LogEvent::Progress(ProgressLog { level: LogLevel::Debug, message })
}

fn importing_done() -> LogEvent {
    LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        stage: Stage::ImportingDone,
    })
}

fn added_root(name: &str, version: &str, dt: DependencyType) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: CWD.to_string(),
            added: AddedRoot {
                name: name.to_string(),
                real_name: name.to_string(),
                version: Some(version.to_string()),
                dependency_type: Some(dt),
                id: None,
                latest: None,
                linked_from: None,
            },
        },
    })
}

fn summary() -> LogEvent {
    LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix: CWD.to_string() })
}

#[test]
fn progress_line_counts_each_status() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            progress("resolved"),
            progress("resolved"),
            progress("resolved"),
            progress("found_in_store"),
            progress("found_in_store"),
            progress("imported"),
        ],
    );
    assert_eq!(frame, "Progress: resolved 3, reused 2, downloaded 0, added 1");
}

#[test]
fn importing_done_appends_done_suffix() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![progress("resolved"), progress("imported"), importing_done()]);
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 1, done");
}

#[test]
fn stats_render_packages_line_and_bar() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added { prefix: CWD.to_string(), added: 5 },
            }),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Removed { prefix: CWD.to_string(), removed: 2 },
            }),
        ],
    );
    assert_eq!(frame, "Packages: +5 -2\n+++++--");
}

#[test]
fn stats_bar_is_colored_when_enabled() {
    let mut reporter = state(true);
    let frame = render(
        &mut reporter,
        vec![LogEvent::Stats(StatsLog {
            level: LogLevel::Debug,
            message: StatsMessage::Added { prefix: CWD.to_string(), added: 1 },
        })],
    );
    assert_eq!(frame, "Packages: \u{1b}[32m+1\u{1b}[39m\n\u{1b}[32m+\u{1b}[39m");
}

#[test]
fn summary_groups_by_dependency_type_in_order() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root("bar", "2.0.0", DependencyType::Dev),
            added_root("foo", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n\ndevDependencies:\n+ bar 2.0.0\n");
}

#[test]
fn context_block_renders_when_no_current_lockfile() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::Context(ContextLog {
                level: LogLevel::Debug,
                current_lockfile_exists: false,
                store_dir: "/store".to_string(),
                virtual_store_dir: "/repo/node_modules/.pnpm".to_string(),
            }),
            LogEvent::PackageImportMethod(PackageImportMethodLog {
                level: LogLevel::Debug,
                method: PackageImportMethod::Hardlink,
            }),
        ],
    );
    assert_eq!(
        frame,
        "Packages are hard linked from the content-addressable store to the virtual store.\n  \
         Content-addressable store is at: /store\n  Virtual store is at:             node_modules/.pnpm",
    );
}

#[test]
fn context_block_suppressed_when_lockfile_exists() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::Context(ContextLog {
                level: LogLevel::Debug,
                current_lockfile_exists: true,
                store_dir: "/store".to_string(),
                virtual_store_dir: "/repo/node_modules/.pnpm".to_string(),
            }),
            LogEvent::PackageImportMethod(PackageImportMethodLog {
                level: LogLevel::Debug,
                method: PackageImportMethod::Hardlink,
            }),
        ],
    );
    assert_eq!(frame, "");
}

#[test]
fn execution_time_renders_done_footer() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::ExecutionTime(ExecutionTimeLog {
            level: LogLevel::Debug,
            started_at: 1000,
            ended_at: 3500,
        })],
    );
    assert!(frame.starts_with("Done in 2.5s using pacquet v"), "got: {frame}");
}

#[test]
fn already_up_to_date_pnpm_log_renders() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: "Already up to date".to_string(),
            prefix: CWD.to_string(),
        })],
    );
    assert_eq!(frame, "Already up to date");
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
         Done in 1.2s using pacquet v0.0.1",
    );
}

#[test]
fn warnings_collapse_after_five() {
    let mut reporter = state(false);
    let warn = || {
        LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: "something".to_string(),
            prefix: CWD.to_string(),
        })
    };
    let events: Vec<LogEvent> = (0..6).map(|_| warn()).collect();
    let frame = render(&mut reporter, events);
    let lines: Vec<&str> = frame.lines().collect();
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[0], "[WARN] something");
    assert_eq!(lines[5], "[WARN] 1 other warnings");
}

#[test]
fn append_only_emits_lines_not_frames() {
    let mut reporter = ReporterState::new(CWD.to_string(), 80, Colors { enabled: false }, true);
    let out = reporter.handle(&progress("resolved"));
    match out {
        Output::Lines(lines) => {
            assert_eq!(lines, vec!["Progress: resolved 1, reused 0, downloaded 0, added 0"]);
        }
        _ => panic!("append-only should emit Lines"),
    }
}

#[test]
fn lifecycle_script_output_is_grouped_and_indented() {
    let mut reporter = state(false);
    let dep_path = "foo@1.0.0";
    let wd = "/repo/deps/foo"; // not under node_modules → not collapsed
    let events = vec![
        LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Script {
                dep_path: dep_path.to_string(),
                optional: false,
                script: "node build.js".to_string(),
                stage: "postinstall".to_string(),
                wd: wd.to_string(),
            },
        }),
        LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Stdio {
                dep_path: dep_path.to_string(),
                line: "building".to_string(),
                stage: "postinstall".to_string(),
                stdio: LifecycleStdio::Stdout,
                wd: wd.to_string(),
            },
        }),
    ];
    let frame = render(&mut reporter, events);
    assert_eq!(frame, "deps/foo postinstall$ node build.js\n│ building\n└─ Running...");
}

fn lockfile_cached() -> LogEvent {
    LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Cached {
            verified_at: None,
            lockfile_path: Some("/repo/pnpm-lock.yaml".to_string()),
        },
    })
}

/// Cached event carrying a `verified_at` timestamp two hours in the past —
/// exercises the "verified Xh ago" suffix path (mirrors the TS test at
/// `pnpm11/cli/default-reporter/test/reportingLockfileVerification.ts`).
fn lockfile_cached_two_hours_ago() -> LogEvent {
    let two_hours_ago = Utc::now() - chrono::Duration::seconds(2 * 3600);
    LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Cached {
            verified_at: Some(two_hours_ago.to_rfc3339()),
            lockfile_path: Some("/repo/pnpm-lock.yaml".to_string()),
        },
    })
}

/// Mirror of the TS regression test in
/// `pnpm11/cli/default-reporter/test/reportingLockfileVerification.ts`:
/// the cached verdict is a one-shot status that must not be re-rendered on
/// every subsequent progress tick. Without the cached-then-clear pattern
/// the line stays in `blocks[]` forever and is re-included in every
/// redraw — producing dozens of duplicate lines in captured output.
#[test]
fn lockfile_cached_does_not_repeat_on_subsequent_progress_frames() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            lockfile_cached(),
            progress("resolved"),
            progress("fetched"),
            progress("found_in_store"),
            progress("imported"),
        ],
    );
    // The cached event's own frame included the verdict line; the next
    // (non-LockfileVerification) event triggers the pre-dispatch clear in
    // `handle()`, so the final frame contains only the rolling progress line.
    assert_eq!(frame, "Progress: resolved 1, reused 1, downloaded 1, added 1");
}

/// The cached verdict still renders at least once — the clear in `handle()`
/// only fires on the *next* event, so a lone cached event produces the
/// verdict line as its frame.
#[test]
fn lockfile_cached_renders_verdict_line_once_in_isolation() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![lockfile_cached()]);
    assert_eq!(frame, "✓ Lockfile passes supply-chain policies (previously verified)");
}

/// In append-only mode the cached event emits its verdict line directly to
/// `pending` (no fixed-block juggling), and the clear hook in `handle()` is
/// a no-op because `slot.fixed` was never set. The result is exactly the
/// two payload lines, with no blank in between.
#[test]
fn lockfile_cached_append_only_does_not_emit_blank_clear_line() {
    let mut reporter = ReporterState::new(CWD.to_string(), 80, Colors { enabled: false }, true);
    let mut lines: Vec<String> = Vec::new();
    for event in [lockfile_cached(), progress("resolved")] {
        if let Output::Lines(emitted) = reporter.handle(&event) {
            lines.extend(emitted);
        }
    }
    // Cached verdict line + progress line, no blanks in between.
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "✓ Lockfile passes supply-chain policies (previously verified)");
    assert_eq!(lines[1], "Progress: resolved 1, reused 0, downloaded 0, added 0");
}

/// When the cached verdict carries a `verified_at` timestamp, the suffix
/// renders the elapsed time compactly (e.g. "verified 2h ago") instead of
/// the timeless "previously verified" — matching pnpm's
/// `formatCachedVerdict`.
#[test]
fn lockfile_cached_with_timestamp_renders_verified_age() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![lockfile_cached_two_hours_ago()]);
    // Two hours + the few ms the test took to get here rounds under
    // `pretty_ms_compact`'s 0.05h threshold (180s), so the suffix stays
    // "2h ago" rather than ticking over to a tenth-of-an-hour readout.
    assert_eq!(frame, "✓ Lockfile passes supply-chain policies (verified 2h ago)");
}

/// A `verified_at` string that doesn't parse as RFC 3339 falls back to the
/// timeless "previously verified" suffix — matches pnpm's NaN-elapsed
/// fallback in `formatCachedVerdict`.
#[test]
fn lockfile_cached_with_unparseable_timestamp_falls_back() {
    let mut reporter = state(false);
    let event = LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Cached {
            verified_at: Some("not-a-timestamp".to_string()),
            lockfile_path: Some("/repo/pnpm-lock.yaml".to_string()),
        },
    });
    let frame = render(&mut reporter, vec![event]);
    assert_eq!(frame, "✓ Lockfile passes supply-chain policies (previously verified)");
}

/// Confirm the test-suite clock assumption above: a `DateTime` two hours
/// before `Utc::now()` parses back to within a few hundred milliseconds of
/// `7_200_000` ms. If this ever drifts (e.g. chrono changes its RFC 3339
/// precision), the `lockfile_cached_with_timestamp_renders_verified_age`
/// assertion will need to be revisited.
#[test]
fn two_hours_ago_parses_close_to_7200s_before_now() {
    let two_hours_ago = Utc::now() - chrono::Duration::seconds(2 * 3600);
    let parsed = DateTime::parse_from_rfc3339(&two_hours_ago.to_rfc3339()).unwrap();
    let elapsed_ms = Utc::now().timestamp_millis() - parsed.timestamp_millis();
    assert!((7_200_000..=7_201_000).contains(&elapsed_ms), "expected ~7200000ms, got {elapsed_ms}");
}

/// `Hook` and `BrokenModules` are explicitly no-op channels in the dispatch
/// match. The pre-dispatch clear in `handle()` must skip them too — otherwise
/// a debug-only event arriving just after the cached verdict would force a
/// redraw whose only effect is to drop the cached line, producing an extra
/// captured frame in CI / `tee` / `script` output. The clear should wait for
/// the next event that actually updates the frame.
#[test]
fn hook_and_broken_modules_do_not_clear_cached_verdict_prematurely() {
    let mut reporter = state(false);

    // Cached event sets the verdict line as the current frame.
    let cached_frame = match reporter.handle(&lockfile_cached()) {
        Output::Frame(f) => f,
        Output::None => panic!("cached event should produce a Frame, got Output::None"),
        Output::Lines(_) => panic!("cached event should produce a Frame, got Output::Lines"),
    };
    assert_eq!(cached_frame, "✓ Lockfile passes supply-chain policies (previously verified)");

    // A debug-only Hook event arrives. It must NOT trigger a redraw just to
    // clear the verdict — Output::None means no frame is emitted.
    let hook = LogEvent::Hook(HookLog {
        level: LogLevel::Debug,
        from: "readPackage".to_string(),
        hook: "readPackage".to_string(),
        message: "ignored".to_string(),
        prefix: CWD.to_string(),
    });
    assert!(matches!(reporter.handle(&hook), Output::None), "Hook should not redraw");

    // Same for BrokenModules.
    let broken = LogEvent::BrokenModules(BrokenModulesLog {
        level: LogLevel::Debug,
        missing: "whatever".to_string(),
    });
    assert!(matches!(reporter.handle(&broken), Output::None), "BrokenModules should not redraw");

    // The next *rendering* event clears the verdict as part of its redraw —
    // the cached line is gone from the final frame.
    let frame = render(&mut reporter, vec![progress("resolved")]);
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 0");
}
