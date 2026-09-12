use super::RunReport;
use crate::cli_args::pipeline::{Selection, SelectionMode};
use std::collections::HashSet;

#[test]
fn empty_selection_upload_has_a_summary_and_an_opaque_run_id() {
    let selection = Selection {
        requested: Vec::new(),
        selected: HashSet::new(),
        mode: SelectionMode::Affected,
        merge_base: None,
        changed_count: 0,
    };
    let pipeline = "../../outside/../pipeline";
    let report = RunReport::new(pipeline, "main", &selection, None).unwrap();
    let upload = report.to_upload("workspace".to_string());
    assert_eq!(upload.summary["pipeline"], pipeline);
    assert_eq!(upload.summary["tasks"], serde_json::json!({}));
    assert_eq!(upload.summary["selection"]["requestedProjects"], 0);
    assert!(upload.run_id.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-'));
    let directory = tempfile::tempdir().unwrap();
    let written = report.write(directory.path()).unwrap();
    assert_eq!(written.parent().unwrap(), directory.path().join("runs"));
    assert_eq!(
        std::fs::read_to_string(written.join("summary.json")).unwrap(),
        serde_json::to_string_pretty(&upload.summary).unwrap(),
    );
}
