//! Frame-level tests: drive sequences of `LogEvent`s through `ReporterState`
//! and assert the rendered output, matching what `@pnpm/cli.default-reporter`
//! produces for the same events. Colors are constructed off for readable
//! plain-text assertions and on for the ANSI-specific ones.

use pnpm_default_reporter::{
    MaxLogLevel, SummaryScope,
    colors::Colors,
    format::pretty_bytes,
    state::{Output, ReporterOptions, ReporterState},
};
use pnpm_reporter::{
    AddedRoot, ContextLog, DedupeCheckLog, DependencyType, DeprecationLog, ExecutionTimeLog,
    FetchingProgressLog, FetchingProgressMessage, GlobalLog, HookLog, IgnoredScriptsLog,
    LifecycleLog, LifecycleMessage, LifecycleStdio, LockfileVerificationLog,
    LockfileVerificationMessage, LogEvent, LogLevel, PackageImportMethod, PackageImportMethodLog,
    PackageManifestLog, PackageManifestMessage, PnpmErrorLog, PnpmLog, ProgressLog,
    ProgressMessage, RootLog, RootMessage, ScopeLog, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalParent, SkippedOptionalReason, Stage, StageLog,
    StatsLog, StatsMessage, SummaryLog, UpdateCheckLog,
};

const CWD: &str = "/repo";

#[test]
fn pretty_bytes_truncates_without_floating_point_drift() {
    assert_eq!(pretty_bytes(1_130), "1.13 kB");
    assert_eq!(pretty_bytes(1_130_000), "1.13 MB");
}

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
    added_root_with_latest_at(prefix, name, version, None, dt)
}

fn added_root_with_latest_at(
    prefix: &str,
    name: &str,
    version: &str,
    latest: Option<&str>,
    dt: DependencyType,
) -> LogEvent {
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
                latest: latest.map(str::to_string),
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
    let first_requester = "/repo/foo";
    let second_requester = "/repo/bar";
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            stage_at(first_requester, Stage::ResolutionStarted),
            progress_at(first_requester, "resolved"),
            stage_at(second_requester, Stage::ResolutionStarted),
            progress_at(second_requester, "resolved"),
        ],
    );
    assert_eq!(
        frame,
        "foo                                      | Progress: resolved 1, reused 0, downloaded 0, added 0\nbar                                      | Progress: resolved 1, reused 0, downloaded 0, added 0",
    );
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
fn summary_prints_is_available_when_latest_is_newer_than_version() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_with_latest_at(CWD, "foo", "1.0.0", Some("2.0.0"), DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0 (2.0.0 is available)\n");
}

#[test]
fn summary_omits_is_available_when_latest_equals_version() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_with_latest_at(CWD, "foo", "3.9.5", Some("3.9.5"), DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 3.9.5\n");
}

#[test]
fn summary_omits_is_available_when_latest_is_older_than_version() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_with_latest_at(CWD, "foo", "2.0.0", Some("1.0.0"), DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 2.0.0\n");
}

#[test]
fn summary_omits_is_available_when_latest_is_not_semver() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_with_latest_at(
                CWD,
                "foo",
                "1.0.0",
                Some("not-a-version"),
                DependencyType::Prod,
            ),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 1.0.0\n");
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

#[test]
fn lockfile_policy_verdict_precedes_the_frozen_install_message() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: "Lockfile is up to date, resolution step is skipped".to_string(),
                prefix: CWD.to_string(),
            }),
            LogEvent::Stage(StageLog {
                level: LogLevel::Debug,
                prefix: CWD.to_string(),
                stage: Stage::ImportingDone,
            }),
            LogEvent::LockfileVerification(LockfileVerificationLog {
                level: LogLevel::Debug,
                message: LockfileVerificationMessage::Cached {
                    verified_at: None,
                    lockfile_path: None,
                },
            }),
        ],
    );
    assert_eq!(
        frame,
        "✓ Lockfile passes supply-chain policies (previously verified)\n\
Lockfile is up to date, resolution step is skipped",
    );
}

#[test]
fn append_only_waits_for_a_terminal_lockfile_policy_verdict() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let pending = reporter.handle(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: CWD.to_string(),
    }));
    assert!(matches!(pending, Output::None));

    let stats = reporter.handle(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { added: 1, prefix: CWD.to_string() },
    }));
    match stats {
        Output::Lines(lines) => {
            assert!(!lines.iter().any(|line| line.contains("Lockfile is up to date")));
        }
        Output::None => {}
        Output::Frame(_) => {
            panic!("install stats should not flush the pending frozen-install message");
        }
    }

    let started = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Started { entries: 2, lockfile_path: None },
    }));
    match started {
        Output::Lines(lines) => {
            assert_eq!(
                lines,
                ["? Verifying lockfile against supply-chain policies (2 entries)..."],
            );
        }
        _ => panic!("started verification should emit only its progress line"),
    }

    let done = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Done {
            entries: 2,
            elapsed_ms: 100,
            lockfile_path: None,
        },
    }));
    match done {
        Output::Lines(lines) => assert_eq!(
            lines,
            [
                "✓ Lockfile passes supply-chain policies (2 entries in 100ms)",
                "Lockfile is up to date, resolution step is skipped",
            ],
        ),
        _ => panic!("completed verification should emit its verdict before the frozen message"),
    }
}

#[test]
fn install_summary_flushes_the_frozen_message_without_a_policy_verdict() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let pending = reporter.handle(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: CWD.to_string(),
    }));
    assert!(matches!(pending, Output::None));

    let summary = reporter.handle(&summary());
    match summary {
        Output::Lines(lines) => {
            dbg!(&lines);
            assert_eq!(lines, ["Lockfile is up to date, resolution step is skipped"]);
        }
        _ => panic!("the install summary should flush the frozen message"),
    }
}

#[test]
fn zero_install_stats_render_already_up_to_date() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added { added: 0, prefix: CWD.to_string() },
            }),
            LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Removed { removed: 0, prefix: CWD.to_string() },
            }),
        ],
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

fn pnpm_log(level: LogLevel, message: &str) -> LogEvent {
    LogEvent::Pnpm(PnpmLog { level, message: message.to_string(), prefix: CWD.to_string() })
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
fn loglevel_error_still_renders_errors() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Error,
        ..ReporterOptions::default()
    });
    let frame = render(&mut reporter, vec![pnpm_log(LogLevel::Error, "ERR_PNPM_FETCH_404")]);
    assert_eq!(frame, "ERR_PNPM_FETCH_404");
}

#[test]
fn loglevel_debug_renders_debug_messages() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Debug,
        ..ReporterOptions::default()
    });
    let frame = render(&mut reporter, vec![pnpm_log(LogLevel::Debug, "resolution details")]);
    assert_eq!(frame, "resolution details");
}

#[test]
fn debug_messages_stay_hidden_at_the_default_loglevel() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![pnpm_log(LogLevel::Debug, "resolution details")]);
    assert_eq!(frame, "");
}

/// Dedupe-check issues are an error-level log upstream
/// (`ERR_PNPM_DEDUPE_CHECK_ISSUES` in `reportError.ts`), so they render
/// at every ceiling, including `error`.
#[test]
fn dedupe_check_issues_render_at_every_loglevel_ceiling() {
    for max_log_level in
        [MaxLogLevel::Error, MaxLogLevel::Warn, MaxLogLevel::Info, MaxLogLevel::Debug]
    {
        let mut reporter =
            state_with_options(ReporterOptions { max_log_level, ..ReporterOptions::default() });
        let frame = render(
            &mut reporter,
            vec![LogEvent::DedupeCheck(DedupeCheckLog {
                level: LogLevel::Error,
                message: "dedupe check issues".to_string(),
                err: PnpmErrorLog {
                    code: "ERR_PNPM_DEDUPE_CHECK_ISSUES".to_string(),
                    message: "dedupe check issues".to_string(),
                },
                dedupe_check_issues: serde_json::Value::Null,
                rendered: "resolution changes".to_string(),
            })],
        );
        println!("ceiling: {max_log_level:?}");
        assert_eq!(frame, "\nresolution changes");
    }
}

#[test]
fn loglevel_warn_renders_warnings_but_not_info() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Warn,
        ..ReporterOptions::default()
    });
    let frame = render(
        &mut reporter,
        vec![
            pnpm_log(LogLevel::Info, "Already up to date"),
            pnpm_log(LogLevel::Warn, "deprecated package"),
        ],
    );
    assert_eq!(frame, "[WARN] deprecated package");
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

fn scope(selected: usize, total: Option<usize>, workspace_prefix: Option<&str>) -> LogEvent {
    LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected,
        total,
        workspace_prefix: workspace_prefix.map(ToString::to_string),
    })
}

fn scope_reporting_state() -> ReporterState {
    state_with_options(ReporterOptions { reports_scope: true, ..ReporterOptions::default() })
}

#[test]
fn reports_an_unnarrowed_workspace_scope() {
    let mut reporter = scope_reporting_state();
    assert_eq!(
        render(&mut reporter, vec![scope(3, Some(3), Some(CWD))]),
        "Scope: all 3 workspace projects",
    );
}

#[test]
fn reports_a_narrowed_workspace_scope() {
    let mut reporter = scope_reporting_state();
    assert_eq!(
        render(&mut reporter, vec![scope(2, Some(3), Some(CWD))]),
        "Scope: 2 of 3 workspace projects",
    );
}

/// Outside a workspace there are no "workspace projects" to count, which
/// is the shape pnpm renders without the qualifier.
#[test]
fn reports_a_scope_without_a_workspace_prefix_as_plain_projects() {
    let mut reporter = scope_reporting_state();
    assert_eq!(render(&mut reporter, vec![scope(2, None, None)]), "Scope: 2 projects");
}

/// A single selected project is the directory the user is standing in, so
/// pnpm says nothing — even for a command that reports scope.
#[test]
fn stays_silent_for_a_single_selected_project() {
    let mut reporter = scope_reporting_state();
    assert!(render(&mut reporter, vec![scope(1, Some(3), Some(CWD))]).is_empty());
}

/// The event fires for every command; only the ones in pnpm's
/// `COMMANDS_THAT_REPORT_SCOPE` render it.
#[test]
fn stays_silent_for_a_command_that_does_not_report_scope() {
    let mut reporter = state(false);
    assert!(render(&mut reporter, vec![scope(3, Some(3), Some(CWD))]).is_empty());
}

// --- embedder reporting options ---------------------------------------

fn ignored_scripts(names: &[&str]) -> LogEvent {
    LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Info,
        package_names: names.iter().map(|name| (*name).to_string()).collect(),
        strict_dep_builds: false,
    })
}

fn linked_root(name: &str, from: &str) -> LogEvent {
    LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: CWD.to_string(),
            added: AddedRoot {
                name: name.to_string(),
                real_name: name.to_string(),
                version: None,
                dependency_type: Some(DependencyType::Prod),
                id: None,
                latest: None,
                linked_from: Some(from.to_string()),
            },
        },
    })
}

#[test]
fn the_ignored_builds_instruction_defaults_to_the_pnpm_command() {
    let mut reporter = state(false);

    let frame = render(&mut reporter, vec![ignored_scripts(&["esbuild"])]);

    assert!(frame.contains("Ignored build scripts: esbuild."), "frame: {frame}");
    assert!(frame.contains(r#"Run "pnpm approve-builds""#), "frame: {frame}");
}

/// An embedder whose users approve builds through its own configuration
/// replaces the instruction line; the list of blocked packages above it
/// is unchanged.
#[test]
fn the_ignored_builds_instruction_can_be_replaced() {
    let mut reporter = state_with_options(ReporterOptions {
        ignored_builds_instruction_text: Some("Set allowScripts in workspace.jsonc.".to_string()),
        ..ReporterOptions::default()
    });

    let frame = render(&mut reporter, vec![ignored_scripts(&["esbuild"])]);

    assert!(frame.contains("Ignored build scripts: esbuild."), "frame: {frame}");
    assert!(frame.contains("Set allowScripts in workspace.jsonc."), "frame: {frame}");
    assert!(!frame.contains("pnpm approve-builds"), "frame: {frame}");
}

fn update_check(current_version: &str, latest_version: &str) -> LogEvent {
    LogEvent::UpdateCheck(UpdateCheckLog {
        level: LogLevel::Debug,
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
    })
}

#[test]
fn a_newer_pnpm_is_announced_with_its_changelog() {
    let mut reporter = state(false);

    let frame = render(&mut reporter, vec![update_check("11.22.0", "12.0.0")]);

    assert!(frame.contains("Update available! 11.22.0 → 12.0.0."), "frame: {frame}");
    assert!(frame.contains("Changelog: https://pnpm.io/v/12.0.0"), "frame: {frame}");
    assert!(frame.contains("To update, run: "), "frame: {frame}");
}

/// The registry's `latest` trails a prerelease build of the next major, so
/// the notice would be an invitation to downgrade.
#[test]
fn nothing_is_announced_unless_the_latest_version_is_ahead() {
    let mut reporter = state(false);

    assert_eq!(render(&mut reporter, vec![update_check("12.0.0", "11.22.0")]), "");
    assert_eq!(render(&mut reporter, vec![update_check("12.0.0", "12.0.0")]), "");
    assert_eq!(render(&mut reporter, vec![update_check("12.0.0-rc.8", "11.22.0")]), "");
}

#[test]
fn linked_packages_appear_in_the_summary_by_default() {
    let mut reporter = state(false);

    let frame = render(&mut reporter, vec![linked_root("@acme/runtime", "/elsewhere"), summary()]);

    assert!(frame.contains("@acme/runtime"), "frame: {frame}");
}

#[test]
fn a_hide_linked_pattern_drops_matching_linked_entries_from_the_summary() {
    let mut reporter = state_with_options(ReporterOptions {
        hide_linked_pkgs_diff: vec!["@acme/*".to_string()],
        ..ReporterOptions::default()
    });

    let frame = render(
        &mut reporter,
        vec![
            linked_root("@acme/runtime", "/elsewhere"),
            linked_root("@other/tool", "/elsewhere"),
            summary(),
        ],
    );

    assert!(!frame.contains("@acme/runtime"), "frame: {frame}");
    assert!(frame.contains("@other/tool"), "frame: {frame}");
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

fn lifecycle_stdio_events() -> Vec<LogEvent> {
    vec![
        LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Script {
                dep_path: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
                optional: false,
                script: "node install.js".to_string(),
                stage: "postinstall".to_string(),
                wd: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
            },
        }),
        LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Stdio {
                dep_path: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
                line: "downloading the binary".to_string(),
                stage: "postinstall".to_string(),
                stdio: LifecycleStdio::Stdout,
                wd: "/repo/node_modules/.pnpm/esbuild@1.0.0".to_string(),
            },
        }),
    ]
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

fn lifecycle_script(wd: &str, stage: &str, script: &str) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Script {
            dep_path: wd.to_string(),
            optional: false,
            script: script.to_string(),
            stage: stage.to_string(),
            wd: wd.to_string(),
        },
    })
}

fn lifecycle_line(wd: &str, stage: &str, line: &str) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Stdio {
            dep_path: wd.to_string(),
            line: line.to_string(),
            stage: stage.to_string(),
            stdio: LifecycleStdio::Stdout,
            wd: wd.to_string(),
        },
    })
}

fn lifecycle_exit(wd: &str, stage: &str, exit_code: i32) -> LogEvent {
    LogEvent::Lifecycle(LifecycleLog {
        level: LogLevel::Debug,
        message: LifecycleMessage::Exit {
            dep_path: wd.to_string(),
            exit_code,
            optional: false,
            stage: stage.to_string(),
            wd: wd.to_string(),
        },
    })
}

/// Two projects whose `postinstall` output interleaves, plus a third
/// that starts and finishes in between.
fn interleaved_lifecycle_events() -> Vec<LogEvent> {
    vec![
        lifecycle_script("/repo/packages/foo", "postinstall", "node foo"),
        lifecycle_line("/repo/packages/foo", "postinstall", "foo I"),
        lifecycle_script("/repo/packages/bar", "postinstall", "node bar"),
        lifecycle_line("/repo/packages/bar", "postinstall", "bar I"),
        lifecycle_line("/repo/packages/foo", "postinstall", "foo II"),
        lifecycle_exit("/repo/packages/bar", "postinstall", 0),
        lifecycle_exit("/repo/packages/foo", "postinstall", 0),
    ]
}

fn emitted_lines(reporter: &mut ReporterState, events: Vec<LogEvent>) -> Vec<String> {
    let mut lines = Vec::new();
    for event in events {
        if let Output::Lines(emitted) = reporter.handle(&event) {
            lines.extend(emitted);
        }
    }
    lines
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

/// Port of upstream's `groups lifecycle output when append-only and
/// reporter-hide-prefix are used`: only the script's own output loses the
/// prefix — the command echo and the `Done` line keep theirs.
#[test]
fn hide_lifecycle_prefix_only_drops_it_from_output_lines() {
    let mut reporter = state_with_options(ReporterOptions {
        append_only: true,
        hide_lifecycle_prefix: true,
        ..ReporterOptions::default()
    });

    let lines = emitted_lines(&mut reporter, interleaved_lifecycle_events());

    assert_eq!(
        lines,
        [
            "packages/foo postinstall$ node foo",
            "foo I",
            "packages/bar postinstall$ node bar",
            "bar I",
            "foo II",
            "packages/bar postinstall: Done",
            "packages/foo postinstall: Done",
        ],
    );
}
