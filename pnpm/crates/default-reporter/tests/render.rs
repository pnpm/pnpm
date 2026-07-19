//! Frame-level tests: drive sequences of `LogEvent`s through `ReporterState`
//! and assert the rendered output, matching what `@pnpm/cli.default-reporter`
//! produces for the same events. Colors are constructed off for readable
//! plain-text assertions and on for the ANSI-specific ones.

use pacquet_default_reporter::{
    SummaryScope,
    colors::Colors,
    state::{Output, ReporterOptions, ReporterState},
};
use pacquet_reporter::{
    AddedRoot, ContextLog, DependencyType, DeprecationLog, ExecutionTimeLog, FetchingProgressLog,
    FetchingProgressMessage, GlobalLog, HookLog, LifecycleLog, LifecycleMessage, LifecycleStdio,
    LogEvent, LogLevel, PackageImportMethod, PackageImportMethodLog, PackageManifestLog,
    PackageManifestMessage, PnpmLog, ProgressLog, ProgressMessage, RootLog, RootMessage,
    SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalParent,
    SkippedOptionalReason, Stage, StageLog, StatsLog, StatsMessage, SummaryLog,
};

const CWD: &str = "/repo";

fn state(colors: bool) -> ReporterState {
    ReporterState::new(CWD.to_string(), 80, Colors { enabled: colors }, false)
}

fn state_with_options(options: ReporterOptions) -> ReporterState {
    ReporterState::new_with_options(CWD.to_string(), 80, Colors { enabled: false }, options)
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
    progress_at(CWD, status)
}

fn progress_at(requester: &str, status: &str) -> LogEvent {
    let requester = requester.to_string();
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

fn stage_at(prefix: &str, stage: Stage) -> LogEvent {
    LogEvent::Stage(StageLog { level: LogLevel::Debug, prefix: prefix.to_string(), stage })
}

fn fetching_started(package_id: &str, size: u64, attempt: u32) -> LogEvent {
    LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt,
            package_id: package_id.to_string(),
            size: Some(size),
        },
    })
}

fn fetching_in_progress(package_id: &str, downloaded: u64) -> LogEvent {
    LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::InProgress {
            downloaded,
            package_id: package_id.to_string(),
        },
    })
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
    summary_at(CWD)
}

fn summary_at(prefix: &str) -> LogEvent {
    LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix: prefix.to_string() })
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
fn prints_progress_beginning() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![stage_at(CWD, Stage::ResolutionStarted), progress("resolved")]);
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 0");
}

#[test]
fn prints_progress_without_added_packages_stats() {
    let mut reporter = state_with_options(ReporterOptions {
        hide_added_pkgs_progress: true,
        ..ReporterOptions::default()
    });
    let frame =
        render(&mut reporter, vec![stage_at(CWD, Stage::ResolutionStarted), progress("resolved")]);
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0");
}

#[test]
fn prints_all_progress_stats() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            stage_at(CWD, Stage::ResolutionStarted),
            progress("resolved"),
            progress("fetched"),
            progress("found_in_store"),
            progress("imported"),
        ],
    );
    assert_eq!(frame, "Progress: resolved 1, reused 1, downloaded 1, added 1");
}

#[test]
fn prints_progress_beginning_for_node_modules_outside_cwd() {
    let requester = "/repo/foo";
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![stage_at(requester, Stage::ResolutionStarted), progress_at(requester, "resolved")],
    );
    assert_eq!(
        frame,
        "foo                                      | Progress: resolved 1, reused 0, downloaded 0, added 0",
    );
}

#[test]
fn hides_progress_prefix_for_node_modules_outside_cwd() {
    let requester = "/repo/foo";
    let mut reporter = state_with_options(ReporterOptions {
        hide_progress_prefix: true,
        ..ReporterOptions::default()
    });
    let frame = render(
        &mut reporter,
        vec![stage_at(requester, Stage::ResolutionStarted), progress_at(requester, "resolved")],
    );
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 0");
}

#[test]
fn prints_progress_beginning_in_append_only_mode() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    assert!(matches!(reporter.handle(&stage_at(CWD, Stage::ResolutionStarted)), Output::None,));
    let Output::Lines(lines) = reporter.handle(&progress("resolved")) else {
        panic!("append-only progress must emit a line");
    };
    assert_eq!(lines, vec!["Progress: resolved 1, reused 0, downloaded 0, added 0"]);
}

#[test]
fn prints_progress_beginning_during_recursive_install() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![stage_at(CWD, Stage::ResolutionStarted), progress("resolved")]);
    assert_eq!(frame, "Progress: resolved 1, reused 0, downloaded 0, added 0");
}

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
fn moves_fixed_progress_line_to_the_end() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            stage_at(CWD, Stage::ResolutionStarted),
            progress("resolved"),
            progress("fetched"),
            LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: "foo".to_string(),
                prefix: CWD.to_string(),
            }),
            stage_at(CWD, Stage::ResolutionDone),
            stage_at(CWD, Stage::ImportingDone),
        ],
    );
    assert_eq!(frame, "[WARN] foo\nProgress: resolved 1, reused 0, downloaded 1, added 0, done");
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
fn append_only_stats_render_once_after_both_events() {
    let mut reporter = ReporterState::new(CWD.to_string(), 80, Colors { enabled: false }, true);
    let added = reporter.handle(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: CWD.to_string(), added: 5 },
    }));
    assert!(matches!(added, Output::None));

    let removed = reporter.handle(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Removed { prefix: CWD.to_string(), removed: 0 },
    }));
    match removed {
        Output::Lines(lines) => assert_eq!(lines, vec!["Packages: +5\n+++++"]),
        _ => panic!("complete stats should emit Lines"),
    }
}

#[test]
fn append_only_stats_render_on_summary_when_pair_is_incomplete() {
    let mut reporter = ReporterState::new(CWD.to_string(), 80, Colors { enabled: false }, true);
    let added = reporter.handle(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: CWD.to_string(), added: 5 },
    }));
    assert!(matches!(added, Output::None));

    let other_summary = reporter.handle(&summary_at("/repo/packages/other"));
    assert!(matches!(other_summary, Output::None));

    let summarized = reporter.handle(&summary());
    match summarized {
        Output::Lines(lines) => assert_eq!(lines, vec!["Packages: +5\n+++++"]),
        _ => panic!("summary should flush incomplete stats"),
    }
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

/// Upstream keeps the console silent for skipped-optional emits without a
/// `parents` chain (build/platform skips), so those must render nothing.
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
            parents: None,
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

/// A resolution-failure skip on a direct optional dependency
/// (`parents: []`, prefix == cwd) renders the same info line as
/// upstream's `reportSkippedOptionalDependencies`; a transitive skip
/// (non-empty `parents`) stays silent.
#[test]
fn skipped_optional_resolution_failure_renders_only_top_level() {
    let skipped = |parents: Vec<SkippedOptionalParent>, prefix: &str| {
        LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
            level: LogLevel::Debug,
            details: Some("No matching version found for broken@^1.0.0".to_string()),
            package: SkippedOptionalPackage::ResolutionFailure {
                name: Some("broken".to_string()),
                version: Some("^1.0.0".to_string()),
                bare_specifier: "^1.0.0".to_string(),
            },
            parents: Some(parents),
            prefix: prefix.to_string(),
            reason: SkippedOptionalReason::ResolutionFailure,
        })
    };

    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![skipped(Vec::new(), CWD)]);
    assert_eq!(
        frame,
        "info: broken@^1.0.0 is an optional dependency and failed compatibility check. Excluding it from installation.",
    );

    let mut reporter = state(false);
    let parent = SkippedOptionalParent {
        id: "parent@1.0.0".to_string(),
        name: "parent".to_string(),
        version: "1.0.0".to_string(),
    };
    let frame = render(&mut reporter, vec![skipped(vec![parent], CWD)]);
    assert!(frame.is_empty(), "transitive skips must not render, got: {frame:?}");

    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![skipped(Vec::new(), "/somewhere/else")]);
    assert!(frame.is_empty(), "other prefixes must not render, got: {frame:?}");
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

fn deprecation(name: &str, version: &str, depth: i32, prefix: &str) -> LogEvent {
    LogEvent::Deprecation(DeprecationLog {
        level: LogLevel::Debug,
        pkg_name: name.to_string(),
        pkg_version: version.to_string(),
        pkg_id: format!("{name}@{version}"),
        prefix: prefix.to_string(),
        deprecated: "no longer supported".to_string(),
        depth,
    })
}

fn resolution_done() -> LogEvent {
    LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        stage: Stage::ResolutionDone,
    })
}

#[test]
fn direct_deprecation_renders_immediately_with_the_message() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![deprecation("express", "0.14.1", 0, CWD)]);
    assert_eq!(frame, "[WARN] deprecated express@0.14.1: no longer supported");
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
        pacquet_default_reporter::format::zoom_out(
            CWD,
            "/repo/packages/app",
            "[WARN] deprecated express@0.14.1",
        ),
    );
}

#[test]
fn transitive_deprecations_flush_as_a_summary_at_resolution_done() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![deprecation("uuid", "3.4.0", 2, CWD), deprecation("request", "2.88.2", 3, CWD)],
    );
    assert!(
        frame.is_empty(),
        "transitive deprecations must buffer until resolution_done: {frame:?}",
    );

    let frame = render(&mut reporter, vec![resolution_done()]);
    assert_eq!(frame, "[WARN] 2 deprecated subdependencies found: request@2.88.2, uuid@3.4.0");
}
