use super::{ImporterDiff, LockfileDiff, render_dry_run_report};

#[test]
fn empty_diff_reports_no_changes() {
    let report = render_dry_run_report(&LockfileDiff::default());
    assert!(report.contains("up to date"), "got: {report}");
    assert!(report.contains("no changes"), "got: {report}");
}

#[test]
fn non_empty_diff_lists_importer_and_package_changes() {
    let diff = LockfileDiff {
        importers: vec![ImporterDiff {
            id: ".".to_string(),
            added: vec![("is-negative".to_string(), "1.0.0".to_string())],
            removed: vec![],
            updated: vec![("is-positive".to_string(), "1.0.0".to_string(), "2.0.0".to_string())],
        }],
        added_packages: vec!["is-negative@1.0.0".to_string()],
        removed_packages: vec![],
    };
    let report = render_dry_run_report(&diff);
    assert!(report.contains("+ is-negative 1.0.0"), "got: {report}");
    assert!(report.contains("is-positive 1.0.0 -> 2.0.0"), "got: {report}");
    assert!(report.contains("+ is-negative@1.0.0"), "got: {report}");
}
