use super::super::manifest_has_bin;
use serde_json::json;

#[test]
fn recognizes_bin_directories_in_package_manifests() {
    assert_eq!(
        manifest_has_bin(Some(&json!({
            "directories": {
                "bin": "cli"
            }
        }))),
        Some(true),
    );
    assert_eq!(manifest_has_bin(Some(&json!({ "directories": { "bin": "" } }))), None);
}
