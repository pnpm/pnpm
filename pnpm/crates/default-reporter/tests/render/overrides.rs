use super::{
    CWD,
    LogEvent,
    LogLevel,
    MaxLogLevel,
    ReporterOptions,
    UnusedOverrideLog,
    render,
    resolution_done,
    state,
    state_with_options,
};

fn unused_override(selector: &str) -> LogEvent {
    LogEvent::UnusedOverride(UnusedOverrideLog {
        level: LogLevel::Debug,
        prefix: CWD.to_string(),
        selector: selector.to_string(),
    })
}

#[test]
fn unused_overrides_buffer_until_resolution_done_then_emit_one_sorted_warning() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![unused_override("parent>child"), unused_override("bar@1.0.0"), unused_override("foo")],
    );
    assert!(frame.is_empty(), "unused overrides must buffer until resolution_done: {frame:?}");

    let frame = render(&mut reporter, vec![resolution_done()]);
    assert_eq!(frame, "[WARN] 3 overrides matched no dependency: bar@1.0.0, foo, parent>child");
}

#[test]
fn a_single_unused_override_uses_the_singular_form() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![unused_override("foo"), resolution_done()]);
    assert_eq!(frame, "[WARN] 1 override matched no dependency: foo");
}

#[test]
fn resolution_done_without_unused_overrides_renders_nothing() {
    let mut reporter = state(false);
    let frame = render(&mut reporter, vec![resolution_done()]);
    assert!(frame.is_empty(), "expected no warning, got: {frame:?}");
}

#[test]
fn unused_override_selectors_are_sanitized_before_rendering() {
    let mut reporter = state(false);
    let frame = render(
        &mut reporter,
        vec![unused_override("inj\u{202E}ect\u{200B}ion\u{2069}\u{FEFF}\x1b"), resolution_done()],
    );
    assert_eq!(frame, "[WARN] 1 override matched no dependency: injection");
}

#[test]
fn unused_overrides_render_at_the_warn_log_level() {
    let mut reporter = state_with_options(ReporterOptions {
        max_log_level: MaxLogLevel::Warn,
        ..ReporterOptions::default()
    });
    let frame = render(&mut reporter, vec![unused_override("foo"), resolution_done()]);
    assert_eq!(frame, "[WARN] 1 override matched no dependency: foo");
}
