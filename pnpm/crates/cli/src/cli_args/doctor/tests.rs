use super::{
    CheckResult, CheckStatus, DoctorReport, can_write_to_dir, check_versions,
    install_probe::last_message, probe_link_capabilities, render_report, status_mark,
};
use pnpm_config::PNPM_VERSION;
use pretty_assertions::assert_eq;

fn report(checks: Vec<CheckResult>) -> DoctorReport {
    DoctorReport { checks }
}

#[test]
fn render_report_summarizes_a_clean_run() {
    let output = render_report(&report(vec![CheckResult::pass("Versions", "pnpm 12.0.0")]));
    dbg!(&output);
    assert_eq!(output, "✓ Versions: pnpm 12.0.0\n\nAll checks passed");
}

/// A warning must not read as a failure, and its fix has to reach the user —
/// a check nobody can act on is noise.
#[test]
fn render_report_shows_the_fix_for_a_warning() {
    let output =
        render_report(&report(vec![CheckResult::warn("Filesystem", "only copying", "Move it.")]));
    dbg!(&output);
    assert_eq!(
        output,
        "‼ Filesystem: only copying\n    Move it.\n\nAll checks passed with 1 warning(s)",
    );
}

#[test]
fn render_report_counts_failures() {
    let output = render_report(&report(vec![
        CheckResult::pass("Versions", "pnpm 12.0.0"),
        CheckResult::fail("Store directory", "no write access to /nope", "Fix it."),
    ]));
    dbg!(&output);
    assert!(output.ends_with("1 check(s) failed"), "{output}");
}

#[test]
fn status_marks_are_distinct() {
    assert_eq!(status_mark(CheckStatus::Pass), "✓");
    assert_eq!(status_mark(CheckStatus::Warn), "‼");
    assert_eq!(status_mark(CheckStatus::Fail), "✗");
}

/// The JSON shape is what the release pipeline and any other tooling read, so
/// it is a contract: camelCase keys, and absent fields omitted rather than null.
#[test]
fn json_report_uses_camel_case_and_omits_empty_fields() {
    let mut check = CheckResult::pass("Filesystem", "available: hardlink");
    check.duration_ms = Some(3);
    let json = serde_json::to_string(&report(vec![check])).expect("serialize report");
    dbg!(&json);
    assert_eq!(
        json,
        r#"{"checks":[{"title":"Filesystem","status":"pass","detail":"available: hardlink","durationMs":3}]}"#,
    );
}

#[test]
fn probe_reports_the_links_a_normal_filesystem_supports() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let capabilities = probe_link_capabilities(dir.path()).expect("probe links");
    dbg!(&capabilities);
    let supported = |name: &str| {
        capabilities
            .iter()
            .any(|(candidate, supported)| *candidate == name && *supported)
    };
    assert!(supported("hardlink"), "a temp dir must support hardlinks");
    assert!(supported("symlink"), "a temp dir must support symlinks");
}

#[test]
fn can_write_to_dir_detects_a_writable_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(can_write_to_dir(dir.path()));
}

#[test]
fn can_write_to_dir_rejects_a_missing_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(!can_write_to_dir(&dir.path().join("does-not-exist")));
}

/// The install smoke test surfaces the last meaningful stderr line, so a
/// trailing blank line must not swallow the actual error.
#[test]
fn last_message_skips_trailing_blanks() {
    assert_eq!(last_message("first\nERR_PNPM_BROKEN  it broke\n\n"), "ERR_PNPM_BROKEN  it broke");
    assert_eq!(last_message(""), "");
}

#[test]
fn last_message_joins_a_wrapped_error_cause() {
    let stderr = "Error: ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED\n\n  × installing dependencies\n  ╰─▶ /p postinstall: `probe` exited with exit\n      status: 127\n\n";

    assert_eq!(last_message(stderr), "/p postinstall: `probe` exited with exit status: 127");
    assert_eq!(last_message("plain failure\n"), "plain failure");
    assert_eq!(last_message(""), "");
}

/// The `cli` crate carries a placeholder `Cargo.toml` version, so the check
/// has to report the released version that `pnpm --version` prints.
#[test]
fn check_versions_reports_the_released_pnpm_version() {
    let detail = check_versions().detail.expect("versions detail");
    assert!(detail.starts_with(&format!("pnpm {PNPM_VERSION}")), "{detail}");
}
