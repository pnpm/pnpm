use super::{
    CWD,
    EngineVersions,
    InstallCheckLog,
    LogEvent,
    LogLevel,
    Output,
    UnsupportedPackage,
    WantedPlatform,
    flush_checks,
    render,
    state,
};

fn install_check(message: &str, unsupported: Option<UnsupportedPackage>) -> LogEvent {
    LogEvent::InstallCheck(InstallCheckLog {
        level: LogLevel::Warn,
        message: message.to_string(),
        prefix: CWD.to_string(),
        unsupported,
    })
}

fn unsupported_node(pkg_id: &str, range: &str) -> LogEvent {
    unsupported_node_on(pkg_id, range, "26.13.0")
}

fn unsupported_node_on(pkg_id: &str, range: &str, current: &str) -> LogEvent {
    install_check(
        &format!("Unsupported engine for {pkg_id}"),
        Some(UnsupportedPackage::Engine {
            pkg_id: pkg_id.to_string(),
            wanted: EngineVersions { node: Some(range.to_string()), pnpm: None },
            current: EngineVersions { node: Some(current.to_string()), pnpm: None },
        }),
    )
}

fn unsupported_platform(pkg_id: &str, wanted: WantedPlatform) -> LogEvent {
    install_check(
        &format!("Unsupported platform for {pkg_id}"),
        Some(UnsupportedPackage::Platform { pkg_id: pkg_id.to_string(), wanted }),
    )
}

fn darwin_only() -> WantedPlatform {
    WantedPlatform { os: Some(vec!["darwin".to_string()]), ..Default::default() }
}

fn footer(events: Vec<LogEvent>) -> String {
    let mut reporter = state(false);
    let frame = render(&mut reporter, events);
    assert_eq!(frame, "", "the checks print nothing before the footer");
    match flush_checks(&mut reporter) {
        Output::Frame(frame) => frame,
        _ => String::new(),
    }
}

#[test]
fn footer_lists_the_packages_that_want_another_node_version() {
    let events = ["a@1.0.0", "b@2.0.0", "c@1.0.0", "d@1.0.0", "b@2.0.0"]
        .map(|pkg_id| unsupported_node(pkg_id, ">=18 <26"))
        .into();
    assert_eq!(
        footer(events),
        "\n✗ 4 packages want Node.js >=18 <26, not 26.13.0: a@1.0.0, b@2.0.0, c@1.0.0 and 1 more\n",
    );
}

#[test]
fn footer_merges_ranges_into_their_intersection() {
    let events = [
        ("a@1.0.0", ">=12.22"),
        ("b@1.0.0", ">=16.0.0"),
        ("c@1.0.0", "^12.22.0 || ^14.17.0 || >=16.0.0"),
        ("d@1.0.0", "^18.0.0 || >=20.0.0"),
        ("e@1.0.0", "^18.18.0 || ^20.9.0 || >=21.1.0"),
    ]
    .map(|(pkg_id, range)| unsupported_node_on(pkg_id, range, "12.0.0"))
    .into();
    assert_eq!(
        footer(events),
        "\n✗ 5 packages want Node.js ^18.18.0 || ^20.9.0 || >=21.1.0, not 12.0.0: \
         a@1.0.0, b@1.0.0, c@1.0.0 and 2 more\n",
    );
}

#[test]
fn footer_keeps_bounds_that_are_not_a_caret_range() {
    let events = vec![
        unsupported_node("a@1.0.0", ">=18.0.0 <26"),
        unsupported_node("b@1.0.0", ">=20"),
        unsupported_node("c@1.0.0", "^0.10.1 || >=20.5.0"),
    ];
    assert_eq!(
        footer(events),
        "\n✗ 3 packages want Node.js >=20.5.0 <26.0.0, not 26.13.0: a@1.0.0, b@1.0.0 and c@1.0.0\n",
    );
}

#[test]
fn footer_joins_adjacent_ranges() {
    let events = vec![
        unsupported_node("a@1.0.0", "^18.0.0 || ^19.0.0 || ^20.0.0 || ^21.0.0 || ^22.0.0"),
        unsupported_node("b@1.0.0", ">=18"),
        unsupported_node("c@1.0.0", "8 || 9 || 10 || 11 || 12"),
    ];
    assert_eq!(
        footer(events),
        "\n✗ 1 package wants Node.js >=8.0.0 <13.0.0, not 26.13.0: c@1.0.0\n\
         ✗ 2 packages want Node.js >=18.0.0 <23.0.0, not 26.13.0: a@1.0.0 and b@1.0.0\n",
    );
}

#[test]
fn footer_keeps_an_explicit_upper_bound_out_of_a_caret_range() {
    let events =
        vec![unsupported_node("a@1.0.0", ">=18.0.0 <19.0.0"), unsupported_node("b@1.0.0", ">=17")];
    assert_eq!(
        footer(events),
        "\n✗ 2 packages want Node.js >=18.0.0 <19.0.0, not 26.13.0: a@1.0.0 and b@1.0.0\n",
    );
}

/// The compact `<26.0.0` would accept the running `26.0.0-rc.1`.
#[test]
fn footer_keeps_prerelease_bounds_that_reject_a_prerelease_node() {
    let events = vec![
        unsupported_node_on("a@1.0.0", ">=18 <26", "26.0.0-rc.1"),
        unsupported_node_on("b@1.0.0", ">=20 <26", "26.0.0-rc.1"),
    ];
    assert_eq!(
        footer(events),
        "\n✗ 2 packages want Node.js >=20.0.0 <26.0.0-0, not 26.0.0-rc.1: a@1.0.0 and b@1.0.0\n",
    );
}

#[test]
fn footer_prints_a_line_per_range_that_has_no_version_in_common() {
    let events = vec![
        unsupported_node("a@1.0.0", ">=18 <26"),
        unsupported_node("b@1.0.0", ">=28"),
        unsupported_node("c@1.0.0", ">=18 <26"),
    ];
    assert_eq!(
        footer(events),
        "\n✗ 2 packages want Node.js >=18 <26, not 26.13.0: a@1.0.0 and c@1.0.0\n\
         ✗ 1 package wants Node.js >=28, not 26.13.0: b@1.0.0\n",
    );
}

#[test]
fn footer_prints_a_range_that_does_not_parse_on_its_own_line() {
    let events = vec![
        unsupported_node_on("a@1.0.0", "not a range", "12.0.0"),
        unsupported_node_on("b@1.0.0", ">=18", "12.0.0"),
        unsupported_node_on("c@1.0.0", ">=20", "12.0.0"),
    ];
    assert_eq!(
        footer(events),
        "\n✗ 2 packages want Node.js >=20.0.0, not 12.0.0: b@1.0.0 and c@1.0.0\n\
         ✗ 1 package wants Node.js not a range, not 12.0.0: a@1.0.0\n",
    );
}

#[test]
fn footer_lists_the_packages_that_want_another_platform() {
    let linux_arm = WantedPlatform {
        os: Some(vec!["linux".to_string(), "android".to_string()]),
        cpu: Some(vec!["arm64".to_string()]),
        libc: Some(Vec::new()),
    };
    assert_eq!(
        footer(vec![
            unsupported_platform("fsevents@2.3.3", darwin_only()),
            unsupported_platform("native@1.0.0", linux_arm),
            unsupported_platform("odd@1.0.0", WantedPlatform::default()),
        ]),
        "\n✗ 1 package wants another platform: odd@1.0.0\n\
         ✗ 1 package wants os darwin: fsevents@2.3.3\n\
         ✗ 1 package wants os linux or android and cpu arm64: native@1.0.0\n",
    );
}

#[test]
fn footer_reads_rejected_platform_values_as_exclusions() {
    let not_darwin_or_linux = WantedPlatform {
        os: Some(vec!["!darwin".to_string(), "!linux".to_string()]),
        ..Default::default()
    };
    assert_eq!(
        footer(vec![unsupported_platform("win-only@1.0.0", not_darwin_or_linux)]),
        "\n✗ 1 package wants os other than darwin or linux: win-only@1.0.0\n",
    );
}

#[test]
fn footer_puts_engine_lines_before_platform_lines() {
    let events = vec![
        unsupported_platform("fsevents@2.3.3", darwin_only()),
        unsupported_node("old@1.0.0", ">=99"),
        install_check(
            "Unsupported engine for tool@1.0.0",
            Some(UnsupportedPackage::Engine {
                pkg_id: "tool@1.0.0".to_string(),
                wanted: EngineVersions {
                    node: Some(">=99".to_string()),
                    pnpm: Some(">=99".to_string()),
                },
                current: EngineVersions {
                    node: Some("26.13.0".to_string()),
                    pnpm: Some("12.0.0".to_string()),
                },
            }),
        ),
    ];
    assert_eq!(
        footer(events),
        "\n✗ 2 packages want Node.js >=99, not 26.13.0: old@1.0.0 and tool@1.0.0\n\
         ✗ 1 package wants pnpm >=99, not 12.0.0: tool@1.0.0\n\
         ✗ 1 package wants os darwin: fsevents@2.3.3\n",
    );
}

#[test]
fn footer_strips_control_characters() {
    assert_eq!(
        footer(vec![unsupported_node("evil\n\u{1b}[2Jpkg@1.0.0", ">=99\u{1b}[2J"),]),
        "\n✗ 1 package wants Node.js >=99[2J, not 26.13.0: evil[2Jpkg@1.0.0\n",
    );
}

#[test]
fn unsupported_packages_are_hidden_below_warn_level() {
    let mut reporter = super::state_with_options(super::ReporterOptions {
        max_log_level: super::MaxLogLevel::Error,
        ..Default::default()
    });
    render(&mut reporter, vec![unsupported_node("a@1.0.0", ">=99")]);
    assert!(matches!(flush_checks(&mut reporter), Output::None));
}

#[test]
fn install_check_without_a_failed_check_warns_at_warn_level_but_not_error_level() {
    for (max_log_level, expected) in [
        (super::MaxLogLevel::Warn, "[WARN] The nodeVersion setting is invalid"),
        (super::MaxLogLevel::Error, ""),
    ] {
        let mut reporter = super::state_with_options(super::ReporterOptions {
            max_log_level,
            ..Default::default()
        });
        let frame =
            render(&mut reporter, vec![install_check("The nodeVersion setting is invalid", None)]);
        assert_eq!(frame, expected);
    }
}

#[test]
fn install_check_warning_strips_control_characters() {
    let mut reporter = state(false);
    let frame =
        render(&mut reporter, vec![install_check("Invalid evil\n\u{1b}[2Jpkg@1.0.0", None)]);
    assert_eq!(frame, "[WARN] Invalid evil[2Jpkg@1.0.0");
}
