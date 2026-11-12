use super::bearer_credentials;

#[test]
fn bearer_credentials_extracts_only_bearer_tokens() {
    assert_eq!(bearer_credentials("Bearer abc123"), Some("abc123"));
    assert_eq!(bearer_credentials("bearer abc123"), Some("abc123")); // case-insensitive
    assert_eq!(bearer_credentials("  Bearer   abc123  "), Some("abc123"));
    assert_eq!(bearer_credentials("Basic dXNlcjpwYXNz"), None);
    assert_eq!(bearer_credentials("abc123"), None);
}
