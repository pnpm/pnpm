use super::{
    current_fields,
    install_input_digest,
    recorded_fields,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::ManifestFile;
use serde_json::json;
use std::path::PathBuf;

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

#[test]
fn install_input_digest_ignores_unchecked_manifest_fields() {
    let manifest = |description: &str, version: &str| {
        PackageManifest::from_value(
            PathBuf::from("package.json"),
            json!({
                "name": "app",
                "dependencies": { "is-positive": version },
                "description": description,
            }),
        )
    };

    assert_eq!(
        install_input_digest(&manifest("first", "1.0.0")),
        install_input_digest(&manifest("edited", "1.0.0")),
    );
    assert_ne!(
        install_input_digest(&manifest("first", "1.0.0")),
        install_input_digest(&manifest("first", "2.0.0")),
    );
}

#[test]
fn install_input_digest_ignores_project_version() {
    let manifest = |version: &str| {
        PackageManifest::from_value(
            PathBuf::from("package.json"),
            json!({
                "name": "app",
                "version": version,
                "dependencies": { "is-positive": "1.0.0" },
            }),
        )
    };

    assert_eq!(install_input_digest(&manifest("1.0.0")), install_input_digest(&manifest("1.0.1")));
}

#[test]
fn install_input_digest_leaves_workspace_bins_to_their_own_check() {
    let manifest = |value: serde_json::Value| {
        PackageManifest::from_value(PathBuf::from("package.json"), value)
    };

    assert_eq!(
        install_input_digest(&manifest(json!({ "name": "app", "bin": { "app-one": "one.js" } }))),
        install_input_digest(&manifest(json!({ "name": "app", "bin": { "app-two": "two.js" } }))),
    );
    assert_eq!(
        install_input_digest(&manifest(
            json!({ "name": "app", "directories": { "bin": "commands" } })
        )),
        install_input_digest(&manifest(
            json!({ "name": "app", "directories": { "bin": "scripts" } })
        )),
    );
}
