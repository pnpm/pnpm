use super::{encode_package_name, escaped_package_name, normalize_registry_url};

#[test]
fn normalize_registry_url_adds_trailing_slash() {
    assert_eq!(normalize_registry_url("https://registry.npmjs.org"), "https://registry.npmjs.org/");
}

#[test]
fn normalize_registry_url_preserves_trailing_slash() {
    assert_eq!(
        normalize_registry_url("https://registry.npmjs.org/"),
        "https://registry.npmjs.org/",
    );
}

#[test]
fn escaped_package_name_lowercases_slash_encoding() {
    assert_eq!(escaped_package_name("lodash"), "lodash");
    assert_eq!(escaped_package_name("@scope/pkg"), "@scope%2fpkg");
}

#[test]
fn encode_package_name_uppercases_slash_encoding() {
    assert_eq!(encode_package_name("lodash"), "lodash");
    assert_eq!(encode_package_name("@scope/pkg"), "@scope%2Fpkg");
}
