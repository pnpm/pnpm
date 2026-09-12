use super::{
    CWD, ContextLog, LockfileVerificationLog, LockfileVerificationMessage, LogEvent, LogLevel,
    Output, PackageImportMethod, PackageImportMethodLog, PnpmLog, ReporterOptions,
    SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalParent,
    SkippedOptionalReason, Stage, StageLog, StatsLog, StatsMessage, deprecation, render,
    resolution_done, state, state_with_options, summary,
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
                ["? Verifying lockfile against supply-chain policies (0/2 entries)..."],
            );
        }
        _ => panic!("started verification should emit only its progress line"),
    }

    let done = reporter.handle(&LogEvent::LockfileVerification(LockfileVerificationLog {
        level: LogLevel::Debug,
        message: LockfileVerificationMessage::Done {
            entries: 2,
            checked: 2,
            elapsed_ms: 100,
            lockfile_path: None,
        },
    }));
    match done {
        Output::Lines(lines) => assert_eq!(
            lines,
            [
                "✓ Lockfile passes supply-chain policies (2/2 entries in 100ms)",
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
