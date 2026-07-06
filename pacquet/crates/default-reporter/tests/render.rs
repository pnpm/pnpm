//! Frame-level tests: drive sequences of `LogEvent`s through `ReporterState`
//! and assert the rendered output, matching what `@pnpm/cli.default-reporter`
//! produces for the same events. Colors are constructed off for readable
//! plain-text assertions and on for the ANSI-specific ones.

use pacquet_default_reporter::{
    SummaryScope,
    colors::Colors,
    state::{Output, ReporterState},
};
use pacquet_reporter::{
    AddedRoot, ContextLog, DependencyType, ExecutionTimeLog, GlobalLog, HookLog, LifecycleLog,
    LifecycleMessage, LifecycleStdio, LogEvent, LogLevel, PackageImportMethod,
    PackageImportMethodLog, PackageManifestLog, PackageManifestMessage, PnpmLog, ProgressLog,
    ProgressMessage, RootLog, RootMessage, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason, Stage, StageLog, StatsLog, StatsMessage, SummaryLog,
};

const CWD: &str = "/repo";

fn state(colors: bool) -> ReporterState {
    ReporterState::new(CWD.to_string(), 80, Colors { enabled: colors }, false)
}

fn state_without_summary_prefix_filter() -> ReporterState {
    ReporterState::new_with_summary_scope(
        CWD.to_string(),
        80,
        Colors { enabled: false },
        false,
        SummaryScope::AllPrefixes,
    )
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
    added_root_at(CWD, name, version, dt)
}

fn added_root_at(prefix: &str, name: &str, version: &str, dt: DependencyType) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: prefix.to_string(),
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

fn package_manifest_initial_at(prefix: &str, value: serde_json::Value) -> LogEvent {
    LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Initial { prefix: prefix.to_string(), initial: value },
    })
}

fn package_manifest_updated_at(prefix: &str, value: serde_json::Value) -> LogEvent {
    LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix: prefix.to_string(), updated: value },
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
fn summary_ignores_root_events_outside_current_prefix() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_at("/repo/packages/foo", "extra", "1.0.0", DependencyType::Prod),
            added_root("foo", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}

#[test]
fn summary_matches_lexically_equivalent_current_prefix() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![added_root_at("/repo/./", "foo", "1.0.0", DependencyType::Prod), summary()],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}

#[test]
fn summary_matches_relative_current_prefix() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![added_root_at(".", "foo", "1.0.0", DependencyType::Prod), summary()],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}

#[test]
fn summary_ignores_manifest_events_outside_current_prefix() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at("/repo/packages/foo", serde_json::json!({})),
            package_manifest_updated_at(
                "/repo/packages/foo",
                serde_json::json!({ "dependencies": { "extra": "^1.0.0" } }),
            ),
            summary(),
        ],
    );
    assert_eq!(frame, "");
}

#[test]
fn empty_summary_does_not_prevent_later_manifest_diff_summary() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at(CWD, serde_json::json!({})),
            summary(),
            package_manifest_updated_at(
                CWD,
                serde_json::json!({ "dependencies": { "foo": "^1.0.0" } }),
            ),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo ^1.0.0\n");
}

#[test]
fn summary_can_include_events_outside_current_prefix() {
    let mut reporter = state_without_summary_prefix_filter();
    let frame = render(
        &mut reporter,
        vec![
            added_root_at("/global/pnpm/packages/foo", "foo", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
}

#[test]
fn summary_keeps_manifest_diffs_separate_when_including_all_prefixes() {
    let mut reporter = state_without_summary_prefix_filter();
    let frame = render(
        &mut reporter,
        vec![
            package_manifest_initial_at("/global/a", serde_json::json!({})),
            package_manifest_updated_at(
                "/global/a",
                serde_json::json!({ "dependencies": { "a": "1.0.0" } }),
            ),
            package_manifest_initial_at("/global/b", serde_json::json!({})),
            package_manifest_updated_at(
                "/global/b",
                serde_json::json!({ "devDependencies": { "b": "2.0.0" } }),
            ),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ a 1.0.0\n\ndevDependencies:\n+ b 2.0.0\n");
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
    assert!(frame.starts_with("Done in 2.5s using pnpm v"), "got: {frame}");
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

/// A `pnpm:global` info message renders as a block, like the prefix-less
/// `pnpm`-channel path — the web-auth flow surfaces the auth URL this way.
#[test]
fn global_info_log_renders() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::Global(GlobalLog {
            level: LogLevel::Info,
            message: "Authenticate your account at:\nhttps://registry.npmjs.org/auth/abc"
                .to_string(),
        })],
    );
    assert_eq!(frame, "Authenticate your account at:\nhttps://registry.npmjs.org/auth/abc");
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
         Done in 1.2s using pnpm v0.0.1",
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

/// Upstream keeps the console silent for the skipped-optional emits pacquet
/// produces (they carry no `parents`), so this channel must render nothing.
#[test]
fn skipped_optional_dependency_renders_nothing() {
    let mut reporter = state(false);
    let skipped = |reason, id: &str, name: &str, version: &str| {
        LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
            level: LogLevel::Debug,
            details: Some("incompatible".to_string()),
            package: SkippedOptionalPackage::Installed {
                id: id.to_string(),
                name: name.to_string(),
                version: version.to_string(),
            },
            prefix: CWD.to_string(),
            reason,
        })
    };
    let frame = render(
        &mut reporter,
        vec![
            skipped(
                SkippedOptionalReason::UnsupportedPlatform,
                "fsevents@2.3.3",
                "fsevents",
                "2.3.3",
            ),
            skipped(SkippedOptionalReason::BuildFailure, "esbuild@0.20.0", "esbuild", "0.20.0"),
        ],
    );
    assert!(frame.is_empty(), "skipped-optional events must not render, got: {frame:?}");
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

#[test]
fn hook_log_renders_with_magenta_hook_name() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::Hook(HookLog {
            level: LogLevel::Info,
            from: "pnpmfile".to_string(),
            hook: "preResolution".to_string(),
            message: "Starting resolution".to_string(),
            prefix: CWD.to_string(),
        })],
    );
    assert_eq!(frame, "preResolution: Starting resolution");
}
