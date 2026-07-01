use super::publish_eligible;
use serde_json::json;

#[test]
fn named_versioned_public_package_is_eligible() {
    let manifest = json!({ "name": "pkg", "version": "1.2.3" });
    assert_eq!(publish_eligible(&manifest), Some(("pkg", "1.2.3")));
}

#[test]
fn private_package_is_skipped() {
    let manifest = json!({ "name": "pkg", "version": "1.2.3", "private": true });
    assert_eq!(publish_eligible(&manifest), None);
}

#[test]
fn explicit_non_private_is_eligible() {
    let manifest = json!({ "name": "pkg", "version": "1.2.3", "private": false });
    assert_eq!(publish_eligible(&manifest), Some(("pkg", "1.2.3")));
}

#[test]
fn missing_name_or_version_is_skipped() {
    assert_eq!(publish_eligible(&json!({ "version": "1.2.3" })), None);
    assert_eq!(publish_eligible(&json!({ "name": "pkg" })), None);
    assert_eq!(publish_eligible(&json!({})), None);
}
