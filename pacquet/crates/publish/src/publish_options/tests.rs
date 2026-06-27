use super::{Access, find_registry_info, resolve_access, scope_of};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn extracts_scope() {
    assert_eq!(scope_of("@scope/pkg"), Some("scope"));
    assert_eq!(scope_of("pkg"), None);
    assert_eq!(scope_of("@scope/"), None);
}

#[test]
fn publish_config_registry_wins() {
    let registry = find_registry_info(
        "@a/b",
        "https://default.example/",
        &BTreeMap::new(),
        Some("https://from-config.example"),
    )
    .unwrap();
    assert_eq!(registry.as_str(), "https://from-config.example/");
}

#[test]
fn scoped_registry_is_used_for_scoped_package() {
    let mut scoped = BTreeMap::new();
    scoped.insert("@a".to_owned(), "https://scoped.example/".to_owned());
    let registry = find_registry_info("@a/b", "https://default.example/", &scoped, None).unwrap();
    assert_eq!(registry.as_str(), "https://scoped.example/");
}

#[test]
fn falls_back_to_default_registry() {
    let registry =
        find_registry_info("pkg", "https://default.example/", &BTreeMap::new(), None).unwrap();
    assert_eq!(registry.as_str(), "https://default.example/");
}

#[test]
fn rejects_unsupported_protocol() {
    let err = find_registry_info("pkg", "ftp://nope.example/", &BTreeMap::new(), None).unwrap_err();
    assert_eq!(err.registry_url, "ftp://nope.example/");
}

#[test]
fn access_prefers_explicit_then_manifest() {
    let manifest = json!({ "publishConfig": { "access": "restricted" } });
    assert_eq!(resolve_access(Some(Access::Public), &manifest), Some(Access::Public));
    assert_eq!(resolve_access(None, &manifest), Some(Access::Restricted));
    assert_eq!(resolve_access(None, &json!({})), None);
}
