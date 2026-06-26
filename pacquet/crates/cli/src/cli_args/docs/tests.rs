use super::is_http_url;

#[test]
fn test_is_http_url_valid_https() {
    assert!(is_http_url("https://example.com"));
}

#[test]
fn test_is_http_url_valid_http() {
    assert!(is_http_url("http://example.com/package"));
}

#[test]
fn test_is_http_url_empty() {
    assert!(!is_http_url(""));
}

#[test]
fn test_is_http_url_non_url() {
    assert!(!is_http_url("not-a-url"));
}

#[test]
fn test_is_http_url_ftp() {
    assert!(!is_http_url("ftp://example.com"));
}

#[test]
fn test_is_http_url_spaces() {
    assert!(!is_http_url("https://exa mple.com"));
}
