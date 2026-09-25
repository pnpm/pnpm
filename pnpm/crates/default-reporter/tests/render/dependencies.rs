use super::{
    CWD, DependencyType, LogEvent, LogLevel, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason, added_root, added_root_with_latest_at, render, state, summary,
    update_check,
};

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

/// The registry's `latest` trails a prerelease build of the next major, so
/// the notice would be an invitation to downgrade.
#[test]
fn nothing_is_announced_unless_the_latest_version_is_ahead() {
    let mut reporter = state(false);

    assert_eq!(render(&mut reporter, vec![update_check("12.0.0", "11.22.0")]), "");
    assert_eq!(render(&mut reporter, vec![update_check("12.0.0", "12.0.0")]), "");
    assert_eq!(render(&mut reporter, vec![update_check("12.0.0-rc.8", "11.22.0")]), "");
}
