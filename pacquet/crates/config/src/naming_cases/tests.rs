use super::{is_camel_case, is_strictly_kebab_case, to_camel_case, to_kebab_case};

#[test]
fn strictly_kebab_case() {
    assert!(is_strictly_kebab_case("fetch-retries"));
    assert!(is_strictly_kebab_case("fetch-retry-mintimeout"));
    assert!(is_strictly_kebab_case("store-dir"));
    // single segment is not strictly kebab-case
    assert!(!is_strictly_kebab_case("storedir"));
    assert!(!is_strictly_kebab_case("registry"));
    // camelCase is not kebab-case
    assert!(!is_strictly_kebab_case("fetchRetries"));
    // leading digit segment is rejected
    assert!(!is_strictly_kebab_case("1-foo"));
    assert!(!is_strictly_kebab_case(""));
}

#[test]
fn camel_case() {
    assert!(is_camel_case("fetchRetries"));
    assert!(is_camel_case("storeDir"));
    assert!(is_camel_case("registry"));
    assert!(!is_camel_case("fetch-retries"));
    assert!(!is_camel_case("FetchRetries"));
    assert!(!is_camel_case("@scope:registry"));
    assert!(!is_camel_case(""));
}

#[test]
fn kebab_conversion() {
    assert_eq!(to_kebab_case("fetchRetries"), "fetch-retries");
    assert_eq!(to_kebab_case("storeDir"), "store-dir");
    assert_eq!(to_kebab_case("fetch-retries"), "fetch-retries");
    assert_eq!(to_kebab_case("httpProxy"), "http-proxy");
    assert_eq!(to_kebab_case("fetchMinSpeedKiBps"), "fetch-min-speed-ki-bps");
    assert_eq!(to_kebab_case("registry"), "registry");
}

#[test]
fn camel_conversion() {
    assert_eq!(to_camel_case("fetch-retries"), "fetchRetries");
    assert_eq!(to_camel_case("store-dir"), "storeDir");
    assert_eq!(to_camel_case("package-extensions"), "packageExtensions");
    assert_eq!(to_camel_case("fetchRetries"), "fetchRetries");
    assert_eq!(to_camel_case("registry"), "registry");
    assert_eq!(to_camel_case("fetch-min-speed-ki-bps"), "fetchMinSpeedKiBps");
}
