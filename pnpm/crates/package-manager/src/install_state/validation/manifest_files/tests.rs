use super::{current_fields, recorded_fields};
use pnpm_workspace_state::ManifestFile;
use serde_json::json;

#[test]
fn records_dev_engines_for_project_host_checks() {
    let manifest = json!({
        "devEngines": { "runtime": { "name": "node", "version": "24.0.0", "onFail": "download" } },
    });

    let fields = recorded_fields(&manifest).expect("recorded fields");

    assert_eq!(fields.get("devEngines"), manifest.get("devEngines"));
}

#[test]
fn refuses_manifest_fields_from_an_old_revision() {
    let file = ManifestFile {
        file_name: "package.json".to_string(),
        mtime_ns: 0,
        ctime_ns: None,
        size: 0,
        fields: Some(
            json!({ "engines": { "node": ">=24" } })
                .as_object()
                .unwrap()
                .clone(),
        ),
        fields_revision: 1,
    };

    assert!(current_fields(&file).is_none());
}
