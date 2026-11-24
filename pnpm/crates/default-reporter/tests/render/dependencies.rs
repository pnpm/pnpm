use super::{
    CWD,
    DependencyType,
    LogEvent,
    LogLevel,
    Output,
    SkippedOptionalDependencyLog,
    SkippedOptionalPackage,
    SkippedOptionalReason,
    added_root,
    added_root_with_latest_at,
    flush_checks,
    linked_root,
    removed_root_at,
    render,
    state,
    summary,
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
fn footer_names_a_dependency_with_a_newer_version() {
    let mut reporter = state(false);
    render(
        &mut reporter,
        vec![
            added_root_with_latest_at(CWD, "foo", "1.0.0", Some("2.0.0"), DependencyType::Prod),
            summary(),
        ],
    );
    let Output::Frame(frame) = flush_checks(&mut reporter) else {
        panic!("the footer renders");
    };
    assert_eq!(
        frame,
        "\ndependencies:\n+ foo 1.0.0\n\n\
         · 1 dependency has a newer version: foo (see \"pnpm outdated\")\n",
    );
}

#[test]
fn footer_omits_a_dependency_when_latest_equals_version() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_with_latest_at(CWD, "foo", "3.9.5", Some("3.9.5"), DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 3.9.5\n");
    assert!(matches!(flush_checks(&mut reporter), Output::None));
}

#[test]
fn footer_omits_a_dependency_when_latest_is_older_than_version() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            added_root_with_latest_at(CWD, "foo", "2.0.0", Some("1.0.0"), DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n+ foo 2.0.0\n");
    assert!(matches!(flush_checks(&mut reporter), Output::None));
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

/// An optional package that could not be fetched is missing from
/// `node_modules`, so the skip renders as a warning with its cause, even
/// when the install runs from a workspace project other than the lockfile
/// directory (<https://github.com/pnpm/pnpm/issues/16514>).
#[test]
fn skipped_optional_fetch_failure_renders_a_warning() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
            level: LogLevel::Debug,
            details: Some("ERR_PNPM_FETCH_404: GET https://registry.example/is-number/-/is-number-7.0.0.tgz: Not Found - 404".to_string()),
            package: SkippedOptionalPackage::Installed {
                id: "is-number@7.0.0".to_string(),
                name: "is-number".to_string(),
                version: "7.0.0".to_string(),
            },
            parents: None,
            prefix: format!("{CWD}/../workspace-root"),
            reason: SkippedOptionalReason::FetchFailure,
        })],
    );
    assert_eq!(
        frame,
        "[WARN] is-number@7.0.0 is an optional dependency that could not be fetched. Excluding it from installation.\nERR_PNPM_FETCH_404: GET https://registry.example/is-number/-/is-number-7.0.0.tgz: Not Found - 404",
    );
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
fn update_of_a_dependency_is_one_line() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            removed_root_at(CWD, "foo", "1.0.0"),
            added_root_with_latest_at(CWD, "foo", "2.0.0", Some("3.0.0"), DependencyType::Prod),
            removed_root_at(CWD, "bar", "2.0.0"),
            added_root("bar", "1.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n↓ bar 2.0.0 → 1.0.0\n↑ foo 1.0.0 → 2.0.0\n");
}

#[test]
fn replacing_a_registry_version_with_a_link_is_two_lines() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![removed_root_at(CWD, "foo", "1.0.0"), linked_root("foo", "/repo/foo"), summary()],
    );
    assert_eq!(frame, "\ndependencies:\n- foo 1.0.0\n+ foo <- foo\n");
}

#[test]
fn update_colors_the_bumped_fields_of_the_new_version() {
    let mut reporter = state(true);
    let frame = render(
        &mut reporter,
        vec![
            removed_root_at(CWD, "foo", "1.2.3"),
            added_root("foo", "1.3.0", DependencyType::Prod),
            removed_root_at(CWD, "bar", "1.2.3"),
            added_root("bar", "2.0.0", DependencyType::Prod),
            summary(),
        ],
    );
    let grey = |text: &str| format!("\u{1b}[90m{text}\u{1b}[39m");
    let line = |name: &str, head: &str, tail: String| {
        let head = if head.is_empty() { String::new() } else { grey(head) };
        format!("\u{1b}[33m↑\u{1b}[39m {name} {}{head}{tail}", grey("1.2.3 → "))
    };
    assert_eq!(
        frame,
        format!(
            "\n\u{1b}[96mdependencies:\u{1b}[39m\n{}\n{}\n",
            line("bar", "", "\u{1b}[31m2.0.0\u{1b}[39m".to_string()),
            line("foo", "1.", "\u{1b}[33m3.0\u{1b}[39m".to_string()),
        ),
    );
}

fn removed_root_with(name: &str, version: &str, registry_package: Option<&str>) -> LogEvent {
    LogEvent::Root(super::RootLog {
        level: LogLevel::Debug,
        message: super::RootMessage::Removed {
            prefix: CWD.to_string(),
            removed: super::RemovedRoot {
                name: name.to_string(),
                version: Some(version.to_string()),
                dependency_type: Some(DependencyType::Prod),
                registry_package: registry_package.map(ToString::to_string),
            },
        },
    })
}

/// A removed link reports its package's manifest version but names no
/// registry package, so a switch to the registry is not an update.
#[test]
fn replacing_a_link_with_a_registry_version_is_two_lines() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            removed_root_with("foo", "1.0.0", None),
            added_root("foo", "1.0.1", DependencyType::Prod),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n- foo 1.0.0\n+ foo 1.0.1\n");
}

#[test]
fn alias_that_names_another_package_is_two_lines() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            removed_root_with("tool", "1.0.0", Some("alpha")),
            super::aliased_root_at(CWD, "tool", "beta", "2.0.0"),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n- tool <- alpha 1.0.0\n+ tool <- beta 2.0.0\n");
}

#[test]
fn update_of_an_alias_target_is_one_line() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![
            removed_root_with("tool", "1.0.0", Some("beta")),
            super::aliased_root_at(CWD, "tool", "beta", "2.0.0"),
            summary(),
        ],
    );
    assert_eq!(frame, "\ndependencies:\n↑ tool <- beta 1.0.0 → 2.0.0\n");
}

/// Names and versions come from the lockfile, which a contributor can edit.
#[test]
fn summary_line_strips_control_characters() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![removed_root_with("tool", "1.0.0\n\u{1b}[2J", Some("evil\n\u{1b}[2Kx")), summary()],
    );
    assert_eq!(frame, "\ndependencies:\n- tool <- evil[2Kx 1.0.0[2J\n");
}

/// A tarball reports its package's version, but names no registry package,
/// so a switch from the registry to it is not an update.
#[test]
fn replacing_a_registry_version_with_a_tarball_is_two_lines() {
    let mut reporter = state(false);
    let tarball = LogEvent::Root(super::RootLog {
        level: LogLevel::Debug,
        message: super::RootMessage::Added {
            prefix: CWD.to_string(),
            added: super::AddedRoot {
                name: "foo".to_string(),
                real_name: "foo".to_string(),
                version: Some("1.0.1".to_string()),
                dependency_type: Some(DependencyType::Prod),
                id: None,
                latest: None,
                linked_from: None,
                registry_package: None,
            },
        },
    });
    let frame = render(
        &mut reporter,
        vec![removed_root_with("foo", "1.0.0", Some("foo")), tarball, summary()],
    );
    assert_eq!(frame, "\ndependencies:\n- foo 1.0.0\n+ foo 1.0.1\n");
}
