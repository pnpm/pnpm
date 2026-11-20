use super::extract_host;

#[test]
fn extract_host_handles_user_authority_and_port() {
    assert_eq!(extract_host("https://github.com/foo/bar"), Some("github.com"));
    assert_eq!(extract_host("git+ssh://git@github.com/foo/bar.git"), Some("github.com"));
    assert_eq!(extract_host("https://host.example:443/foo"), Some("host.example"));
    assert_eq!(extract_host("file:///tmp/x"), None);
    assert_eq!(extract_host("relative/path"), None);
}
