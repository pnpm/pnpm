use super::{censor_protected_settings, is_setting_protected};
use serde_json::json;

#[test]
fn protected_predicate() {
    assert!(is_setting_protected("username"));
    assert!(is_setting_protected("_auth"));
    assert!(is_setting_protected("_authToken"));
    assert!(is_setting_protected("_password"));
    assert!(is_setting_protected("//my-org.example.com:username"));
    assert!(is_setting_protected("//registry.example.com/:_authToken"));
    assert!(!is_setting_protected("registry"));
    assert!(!is_setting_protected("@my-org:registry"));
    assert!(!is_setting_protected("store-dir"));
}

#[test]
fn censors_in_place() {
    let mut map = json!({
        "storeDir": "~/store",
        "username": "general-username",
        "@my-org:registry": "https://my-org.example.com/registry",
        "//my-org.example.com:username": "my-username-in-my-org",
    })
    .as_object()
    .unwrap()
    .clone();

    censor_protected_settings(&mut map);

    assert_eq!(map["storeDir"], json!("~/store"));
    assert_eq!(map["@my-org:registry"], json!("https://my-org.example.com/registry"));
    assert_eq!(map["username"], json!("(protected)"));
    assert_eq!(map["//my-org.example.com:username"], json!("(protected)"));
}

#[test]
fn censors_nested_cache_signing_keys() {
    let mut map = json!({
        "remoteCache": {
            "url": "https://cache.example.com",
            "privateKey": "secret-a"
        },
        "remoteSideEffectsCache": {
            "org": "acme",
            "packages": ["pkg"],
            "privateKey": "secret-b"
        },
        "sideEffectsCache": {
            "read": true,
            "remote": {
                "org": "acme",
                "packages": ["pkg"],
                "privateKey": "secret-c"
            }
        }
    })
    .as_object()
    .unwrap()
    .clone();

    censor_protected_settings(&mut map);

    assert_eq!(map["remoteCache"]["url"], json!("https://cache.example.com"));
    assert_eq!(map["remoteCache"]["privateKey"], json!("(protected)"));
    assert_eq!(map["remoteSideEffectsCache"]["org"], json!("acme"));
    assert_eq!(map["remoteSideEffectsCache"]["privateKey"], json!("(protected)"));
    assert_eq!(map["sideEffectsCache"]["read"], json!(true));
    assert_eq!(map["sideEffectsCache"]["remote"]["privateKey"], json!("(protected)"));
}
