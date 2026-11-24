use super::{
    CWD,
    ContextLog,
    LockfileVerificationLog,
    LockfileVerificationMessage,
    LogEvent,
    LogLevel,
    Output,
    PackageImportMethod,
    PackageImportMethodLog,
    PnpmLog,
    ReporterOptions,
    SkippedOptionalDependencyLog,
    SkippedOptionalPackage,
    SkippedOptionalParent,
    SkippedOptionalReason,
    Stage,
    StageLog,
    StatsLog,
    StatsMessage,
    deprecation,
    flush_checks,
    render,
    resolution_done,
    state,
    state_with_options,
    summary,
};

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
fn passing_lockfile_policy_verdict_moves_to_the_checks_footer() {
    let mut reporter = state(false);
    render(
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
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(
        frame,
        "\n✓ Lockfile passes supply-chain policies\n\
         · Lockfile is up to date, resolution step is skipped\n",
    );
}

/// A skipped resolution is why the deprecation and peer checks are missing
/// from the footer, so the footer says so.
#[test]
fn skipped_resolution_is_explained_in_the_checks_footer() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: "Lockfile is up to date, resolution step is skipped".to_string(),
            prefix: CWD.to_string(),
        })],
    );
    assert_eq!(frame, "");
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(frame, "\n· Lockfile is up to date, resolution step is skipped\n");
}

#[test]
fn in_place_verification_block_is_cleared_by_a_passing_verdict() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::LockfileVerification(LockfileVerificationLog {
                level: LogLevel::Debug,
                message: LockfileVerificationMessage::Started { entries: 2, lockfile_path: None },
            }),
            LogEvent::LockfileVerification(LockfileVerificationLog {
                level: LogLevel::Debug,
                message: LockfileVerificationMessage::Done {
                    entries: 2,
                    checked: 2,
                    elapsed_ms: 100,
                    lockfile_path: None,
                },
            }),
        ],
    );
    assert_eq!(frame, "");
}

#[test]
fn append_only_holds_the_skipped_resolution_for_the_footer() {
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
            assert!(
                !lines
                    .iter()
                    .any(|line| line.contains("Lockfile is up to date")),
            );
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
    assert!(matches!(started, Output::None), "a line append-only output cannot erase");

    let done = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Done {
            entries: 2,
            checked: 2,
            elapsed_ms: 100,
            lockfile_path: None,
        },
    }));
    assert!(matches!(done, Output::None), "a passing verdict goes to the checks footer");
}

#[test]
fn in_place_verification_block_shows_running_progress() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            LogEvent::LockfileVerification(LockfileVerificationLog {
                level: LogLevel::Debug,
                message: LockfileVerificationMessage::Started { entries: 12, lockfile_path: None },
            }),
            LogEvent::LockfileVerification(LockfileVerificationLog {
                level: LogLevel::Debug,
                message: LockfileVerificationMessage::Progress {
                    entries: 12,
                    checked: 7,
                    lockfile_path: None,
                },
            }),
        ],
    );
    assert_eq!(frame, "? Verifying lockfile against supply-chain policies (7/12 entries)...");
}

#[test]
fn append_only_prints_no_passing_verification() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let started = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Started { entries: 12, lockfile_path: None },
    }));
    assert!(matches!(started, Output::None), "a line append-only output cannot erase");

    let progress = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Progress {
            entries: 12,
            checked: 7,
            lockfile_path: None,
        },
    }));
    assert!(matches!(progress, Output::None), "progress must not flood append-only output");

    let done = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Done {
            entries: 12,
            checked: 12,
            elapsed_ms: 100,
            lockfile_path: None,
        },
    }));
    assert!(matches!(done, Output::None), "a passing verdict goes to the checks footer");
}

#[test]
fn failure_verdict_renders_checked_and_total_counts() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::LockfileVerification(LockfileVerificationLog {
            level: LogLevel::Debug,
            message: LockfileVerificationMessage::Failed {
                entries: 12,
                checked: 7,
                elapsed_ms: 100,
                lockfile_path: None,
            },
        })],
    );
    assert_eq!(frame, "✗ Lockfile failed supply-chain policy check (7/12 entries in 100ms)");
}

#[test]
fn append_only_renders_failure_with_checked_count() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let failed = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Failed {
            entries: 12,
            checked: 7,
            elapsed_ms: 100,
            lockfile_path: None,
        },
    }));
    match failed {
        Output::Lines(lines) => {
            assert_eq!(lines.len(), 1);
            assert_eq!(
                lines[0],
                "✗ Lockfile failed supply-chain policy check (7/12 entries in 100ms)",
            );
        }
        _ => panic!("the failure verdict must always render"),
    }
}

#[test]
fn append_only_prints_the_skipped_resolution_with_the_footer() {
    let mut reporter =
        state_with_options(ReporterOptions { append_only: true, ..ReporterOptions::default() });
    let pending = reporter.handle(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: CWD.to_string(),
    }));
    assert!(matches!(pending, Output::None));
    assert!(matches!(reporter.handle(&summary()), Output::None));
    match flush_checks(&mut reporter) {
        Output::Lines(lines) => {
            assert_eq!(lines, ["\n· Lockfile is up to date, resolution step is skipped\n"]);
        }
        _ => panic!("the footer should print the skipped resolution"),
    }
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
        "info: broken@^1.0.0 is an optional dependency that could not be resolved. Excluding it from installation.",
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
fn transitive_deprecations_wait_for_the_checks_footer() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            deprecation("uuid", "3.4.0", 2, CWD),
            deprecation("request", "2.88.2", 3, CWD),
            resolution_done(),
        ],
    );
    assert!(frame.is_empty(), "deprecations must wait for the footer: {frame:?}");

    let frame = render(&mut reporter, vec![summary()]);
    assert!(frame.is_empty(), "deprecations must wait for the footer: {frame:?}");
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(
        frame,
        "\n✗ 2 packages are deprecated: request@2.88.2 and uuid@3.4.0 (see \"pnpm audit deprecations\")\n",
    );
}
