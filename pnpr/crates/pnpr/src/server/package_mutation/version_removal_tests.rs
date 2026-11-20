use super::drop_version;
use serde_json::json;

#[test]
fn removing_a_version_retargets_only_latest() {
    let mut document = json!({
        "versions": { "1.0.0": {}, "2.0.0": {} },
        "dist-tags": { "latest": "2.0.0", "beta": "2.0.0", "stable": "1.0.0" }
    });
    assert!(drop_version(document.as_object_mut().unwrap(), "2.0.0"));
    assert_eq!(document["dist-tags"], json!({ "latest": "1.0.0", "stable": "1.0.0" }));
    assert!(drop_version(document.as_object_mut().unwrap(), "1.0.0"));
    assert_eq!(document["dist-tags"], json!({}));
}
