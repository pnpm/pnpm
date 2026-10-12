use super::{
    canonicalize_npm_registry_url, encode_package_name, escaped_package_name,
    normalize_registry_url,
};

#[test]
fn canonicalize_npm_registry_url_preserves_other_key_spellings() {
    for registry in
        ["//registry.example/custom", "//registry.npmjs.com/custom", "http://registry.npmjs.com"]
    {
        assert_eq!(canonicalize_npm_registry_url(registry), registry);
    }
}

#[test]
fn normalize_registry_url_canonicalizes_npm_alias() {
    for registry in ["https://registry.npmjs.com", "https://registry.npmjs.com/"] {
        assert_eq!(normalize_registry_url(registry), "https://registry.npmjs.org/");
    }
    for registry in ["//registry.npmjs.com", "//registry.npmjs.com/"] {
        assert_eq!(normalize_registry_url(registry), "//registry.npmjs.org/");
    }
}

#[test]
fn normalize_registry_url_preserves_other_endpoints() {
    for registry in [
        "http://registry.npmjs.com/",
        "https://registry.npmjs.com:8443/",
        "https://registry.npmjs.com/custom/",
        "https://registry.npmjs.com.evil.example/",
        "https://registry.npmjs.com@evil.example/",
        "https://evil.example/registry.npmjs.com/",
        "https://registry.npmjs.com/?query=1/",
        "https://registry.npmjs.com/#fragment/",
        "//registry.npmjs.com/custom/",
    ] {
        assert_eq!(normalize_registry_url(registry), registry);
    }
}

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
