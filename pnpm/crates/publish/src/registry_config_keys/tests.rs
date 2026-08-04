use super::{all_registry_config_keys, parse_supported_registry_url};
use pretty_assertions::assert_eq;

#[test]
fn rejects_unsupported_protocol() {
    assert_eq!(parse_supported_registry_url("ftp://example.com"), None);
}

#[test]
fn normalizes_url_and_derives_config_key() {
    let info = parse_supported_registry_url("https://registry.npmjs.org").unwrap();
    assert_eq!(info.normalized_url.as_str(), "https://registry.npmjs.org/");
    assert_eq!(info.longest_config_key.as_str(), "//registry.npmjs.org/");
}

#[test]
fn keeps_existing_trailing_slash() {
    let info = parse_supported_registry_url("http://localhost:4873/path/").unwrap();
    assert_eq!(info.normalized_url.as_str(), "http://localhost:4873/path/");
    assert_eq!(info.longest_config_key.as_str(), "//localhost:4873/path/");
}

#[test]
fn enumerates_keys_longest_to_shortest() {
    let info = parse_supported_registry_url("https://host/a/b/").unwrap();
    let keys: Vec<_> = all_registry_config_keys(&info.longest_config_key)
        .iter()
        .map(|key| key.as_str().to_owned())
        .collect();
    assert_eq!(keys, vec!["//host/a/b/", "//host/a/", "//host/"]);
}

#[test]
fn single_key_for_bare_host() {
    let info = parse_supported_registry_url("https://host").unwrap();
    let keys: Vec<_> = all_registry_config_keys(&info.longest_config_key)
        .iter()
        .map(|key| key.as_str().to_owned())
        .collect();
    assert_eq!(keys, vec!["//host/"]);
}
