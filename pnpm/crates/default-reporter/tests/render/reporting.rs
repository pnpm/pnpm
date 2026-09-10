use super::{
    CWD, Colors, DedupeCheckLog, DependencyType, ExecutionTimeLog, GlobalLog, HookLog,
    LifecycleLog, LifecycleMessage, LifecycleStdio, LogEvent, LogLevel, MaxLogLevel, Output,
    PnpmErrorLog, PnpmLog, ReporterOptions, ReporterState, Stage, StatsLog, StatsMessage,
    added_root_at, added_root_with_latest_at, deprecation, emitted_lines,
    interleaved_lifecycle_events, linked_root, pnpm_log, progress, progress_at, render, scope,
    stage_at, state, state_with_options, summary, summary_at, update_check,
};

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

#[test]
fn direct_deprecation_renders_immediately_with_the_message() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![deprecation("express", "0.14.1", 0, CWD)]);
    assert_eq!(frame, "[WARN] deprecated express@0.14.1: no longer supported");
}

/// The event fires for every command; only the ones in pnpm's
/// `COMMANDS_THAT_REPORT_SCOPE` render it.
#[test]
fn stays_silent_for_a_command_that_does_not_report_scope() {
    let mut reporter = state(false);
    assert!(render(&mut reporter, vec![scope(3, Some(3), Some(CWD))]).is_empty());
}

#[test]
fn a_newer_pnpm_is_announced_with_its_changelog() {
    let mut reporter = state(false);

    let frame = render(&mut reporter, vec![update_check("11.22.0", "12.0.0")]);

    assert!(frame.contains("Update available! 11.22.0 → 12.0.0."), "frame: {frame}");
    assert!(frame.contains("Changelog: https://pnpm.io/v/12.0.0"), "frame: {frame}");
    assert!(frame.contains("To update, run: "), "frame: {frame}");
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
