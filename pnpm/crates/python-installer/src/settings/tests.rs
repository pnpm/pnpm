use super::{parse_index, python_index};
use pnpm_config::{Config, Ecosystem};

fn config_with_pypi_indexes(indexes: &[&str]) -> Config {
    let mut config = Config::default();
    if !indexes.is_empty() {
        config.indexes_by_ecosystem.insert(
            Ecosystem::Pypi,
            indexes
                .iter()
                .map(|index| (*index).to_string())
                .collect(),
        );
    }
    config
}

#[test]
fn the_declared_indexes_are_read_in_order_with_the_default_one_first() {
    let config = config_with_pypi_indexes(&[
        "https://example.test/simple/",
        "https://extra.test/simple/",
        "https://other.test/simple/",
    ]);
    let index = python_index(&config).unwrap();
    assert_eq!(index.url.as_str(), "https://example.test/simple/");
    let extras: Vec<&str> = index.extra_urls
        .iter()
        .map(url::Url::as_str)
        .collect();
    assert_eq!(extras, ["https://extra.test/simple/", "https://other.test/simple/"]);
}

#[test]
fn a_configuration_naming_no_index_resolves_from_pypi() {
    let index = python_index(&config_with_pypi_indexes(&[])).unwrap();
    assert_eq!(index.url.as_str(), pnpm_config::DEFAULT_PYPI_INDEX_URL);
    assert!(index.extra_urls.is_empty());
}

#[test]
fn an_index_credential_reaches_the_index_it_was_configured_for_and_no_other_origin() {
    let mut config =
        config_with_pypi_indexes(&["https://example.test/simple/", "https://other.test/simple/"]);
    config.auth_headers = std::sync::Arc::new(pnpm_network::AuthHeaders::from_creds_map([(
        "//example.test/simple/".to_string(),
        "Bearer index-token".to_string(),
    )]));
    let index = python_index(&config).unwrap();
    assert_eq!(
        index.auth.for_secure_url("https://example.test/simple/alpha/"),
        Some("Bearer index-token".to_string()),
    );
    assert_eq!(index.auth.for_secure_url("https://other.test/simple/alpha/"), None);
}

#[test]
fn a_credential_is_not_sent_over_an_insecure_transport() {
    let mut config = config_with_pypi_indexes(&["http://example.test/simple/"]);
    config.auth_headers = std::sync::Arc::new(pnpm_network::AuthHeaders::from_creds_map([(
        "//example.test/simple/".to_string(),
        "Bearer index-token".to_string(),
    )]));
    let index = python_index(&config).unwrap();
    assert_eq!(index.auth.for_secure_url("http://example.test/simple/alpha/"), None);
}

/// Every Simple API request joins the distribution onto the index, which
/// replaces the last segment when the base carries no trailing slash.
#[test]
fn an_index_without_a_trailing_slash_keeps_its_path() {
    let index = parse_index("https://example.test/simple").unwrap();
    assert_eq!(index.as_str(), "https://example.test/simple/");
    assert_eq!(index.join("alpha/").unwrap().as_str(), "https://example.test/simple/alpha/");
}

/// `registries` refuses a credential in a key, and this refuses one that
/// reaches here any other way, so no Python index carries its own.
#[test]
fn an_index_that_is_not_a_credentialless_http_url_is_refused() {
    for index in ["ftp://example.test/simple/", "https://user:secret@example.test/simple/"] {
        let error = parse_index(index).unwrap_err().to_string();
        assert!(error.contains("HTTP(S) URLs without embedded credentials"), "{index}: {error}");
        assert!(!error.contains("secret"), "{index}: {error}");
    }
}
