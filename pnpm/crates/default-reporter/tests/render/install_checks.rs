use super::{
    CWD,
    EngineVersions,
    InstallCheckLog,
    LogEvent,
    LogLevel,
    Output,
    UnsupportedPackage,
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

fn unsupported_node(pkg_id: &str, current: &str) -> LogEvent {
    install_check(
        &format!("Unsupported engine for {pkg_id}"),
        Some(UnsupportedPackage::Engine {
            pkg_id: pkg_id.to_string(),
            wanted: EngineVersions { node: Some(">=99".to_string()), pnpm: None },
            current: EngineVersions { node: Some(current.to_string()), pnpm: None },
        }),
    )
}

fn unsupported_platform(pkg_id: &str) -> LogEvent {
    install_check(
        &format!("Unsupported platform for {pkg_id}"),
        Some(UnsupportedPackage::Platform { pkg_id: pkg_id.to_string() }),
    )
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
fn footer_lists_the_packages_that_do_not_support_the_node_version() {
    let events = ["a@1.0.0", "b@2.0.0", "c@1.0.0", "d@1.0.0", "b@2.0.0"]
        .map(|pkg_id| unsupported_node(pkg_id, "26.13.0"))
        .into();
    assert_eq!(
        footer(events),
        "\n✗ 4 packages don't support Node.js 26.13.0: a@1.0.0, b@2.0.0, c@1.0.0 and 1 more\n",
    );
}

#[test]
fn footer_lists_the_packages_that_do_not_support_the_platform() {
    assert_eq!(
        footer(vec![unsupported_platform("fsevents@2.3.3")]),
        "\n✗ 1 package doesn't support this platform: fsevents@2.3.3\n",
    );
}

#[test]
fn footer_puts_engine_lines_before_the_platform_line() {
    let events = vec![
        unsupported_platform("fsevents@2.3.3"),
        unsupported_node("old@1.0.0", "26.13.0"),
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
        "\n✗ 2 packages don't support Node.js 26.13.0: old@1.0.0 and tool@1.0.0\n\
         ✗ 1 package doesn't support pnpm 12.0.0: tool@1.0.0\n\
         ✗ 1 package doesn't support this platform: fsevents@2.3.3\n",
    );
}

#[test]
fn footer_strips_control_characters_from_package_ids() {
    assert_eq!(
        footer(vec![unsupported_platform("evil\n\u{1b}[2Jpkg@1.0.0")]),
        "\n✗ 1 package doesn't support this platform: evil[2Jpkg@1.0.0\n",
    );
}

#[test]
fn unsupported_packages_are_hidden_below_warn_level() {
    let mut reporter = super::state_with_options(super::ReporterOptions {
        max_log_level: super::MaxLogLevel::Error,
        ..Default::default()
    });
    render(&mut reporter, vec![unsupported_node("a@1.0.0", "26.13.0")]);
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
