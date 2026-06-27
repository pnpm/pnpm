use super::{is_config_file_key, is_ini_config_key, is_type_key, type_includes_number};

#[test]
fn type_membership() {
    assert!(is_type_key("fetch-retries"));
    assert!(is_type_key("store-dir"));
    assert!(is_type_key("registry"));
    assert!(is_type_key("virtual-store-dir"));
    assert!(!is_type_key("no-such-setting"));
    // prototype-chain names must not be members
    assert!(!is_type_key("constructor"));
    assert!(!is_type_key("__proto__"));
}

#[test]
fn numeric_types() {
    assert!(type_includes_number("fetch-retries"));
    assert!(type_includes_number("fetch-timeout"));
    assert!(type_includes_number("modules-cache-max-age"));
    assert!(type_includes_number("dlx-cache-max-age"));
    assert!(type_includes_number("umask"));
    assert!(!type_includes_number("store-dir"));
    assert!(!type_includes_number("registry"));
    assert!(!type_includes_number("minimum-release-age-strict"));
}

#[test]
fn ini_keys() {
    assert!(is_ini_config_key("registry"));
    assert!(is_ini_config_key("cafile"));
    assert!(is_ini_config_key("_authToken"));
    assert!(is_ini_config_key("username"));
    assert!(is_ini_config_key("@scope:registry"));
    assert!(is_ini_config_key("//registry.example.com/:_auth"));
    // not auth/scoped/registry keys
    assert!(!is_ini_config_key("store-dir"));
    assert!(!is_ini_config_key("fetch-retries"));
    assert!(!is_ini_config_key("https-proxy"));
    assert!(!is_ini_config_key("no-proxy"));
}

#[test]
fn config_file_keys() {
    // pnpm config-file keys
    assert!(is_config_file_key("store-dir"));
    assert!(is_config_file_key("fetch-timeout"));
    assert!(is_config_file_key("cache-dir"));
    // npm-compatible, not excluded
    assert!(is_config_file_key("fetch-retries"));
    assert!(is_config_file_key("registry"));
    // excluded workspace-only / CLI keys
    assert!(!is_config_file_key("catalog-mode"));
    assert!(!is_config_file_key("node-linker"));
    assert!(!is_config_file_key("hoist"));
    assert!(!is_config_file_key("lockfile"));
    // catalog / package-extensions are not even type keys → not config-file keys
    assert!(!is_config_file_key("catalog"));
    assert!(!is_config_file_key("package-extensions"));
}
