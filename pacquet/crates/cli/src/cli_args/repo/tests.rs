use super::*;

#[test]
fn test_opens_repository_url_from_local_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test-pkg", "repository": "https://github.com/test/pkg"}"#,
    )
    .unwrap();
    let url = get_repo_url_from_current_project(dir.path());
    assert_eq!(url.unwrap(), "https://github.com/test/pkg");
}

#[test]
fn test_opens_repository_object_url_from_local_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name": "test-pkg", "repository": {"url": "https://github.com/test/pkg"}}"#,
    )
    .unwrap();
    let url = get_repo_url_from_current_project(dir.path());
    assert_eq!(url.unwrap(), "https://github.com/test/pkg");
}

#[test]
fn test_normalizes_git_plus_https_repository_url_with_git_suffix() {
    let result = repository_to_web_url("git+https://github.com/test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_trims_trailing_slash_from_repository_url() {
    let result = repository_to_web_url("https://github.com/test/pkg/", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_resolves_repository_shorthand_owner_repo() {
    let result = repository_to_web_url("test/pkg", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_resolves_github_shorthand() {
    let result = repository_to_web_url("github:test/pkg", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_resolves_github_shorthand_with_git_suffix() {
    let result = repository_to_web_url("github:test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_resolves_gitlab_shorthand_with_git_suffix() {
    let result = repository_to_web_url("gitlab:test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://gitlab.com/test/pkg"));
}

#[test]
fn test_resolves_git_ssh_repository_url() {
    let result = repository_to_web_url("git+ssh://git@github.com/test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_resolves_scp_style_ssh_url() {
    let result = repository_to_web_url("git@github.com:test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_resolves_scp_style_ssh_url_with_branch() {
    let result = repository_to_web_url("git@github.com:test/pkg.git#main", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/main"));
}

#[test]
fn test_resolves_gitlab_shorthand() {
    let result = repository_to_web_url("gitlab:test/pkg", None);
    assert_eq!(result.as_deref(), Some("https://gitlab.com/test/pkg"));
}

#[test]
fn test_handles_git_slash_trailing() {
    let result = repository_to_web_url("git+https://github.com/test/pkg.git/", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_uses_fragment_as_branch_in_repository_url() {
    let result = repository_to_web_url("git+https://github.com/test/pkg.git#main", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/main"));
}

#[test]
fn test_appends_directory_for_monorepo_packages() {
    let result = repository_to_web_url("https://github.com/test/pkg", Some("packages/foo"));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/master/packages/foo"));
}

#[test]
fn test_resolves_shorthand_with_directory_for_monorepo() {
    let result = repository_to_web_url("test/pkg", Some("packages/bar"));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/master/packages/bar"));
}

#[test]
fn test_combines_directory_and_fragment_in_repository_url() {
    let result = repository_to_web_url("https://github.com/test/pkg#main", Some("packages/foo"));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/main/packages/foo"));
}

#[test]
fn test_combines_directory_and_fragment_in_shorthand() {
    let result = repository_to_web_url("github:test/pkg#main", Some("packages/foo"));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/main/packages/foo"));
}

#[test]
fn test_normalizes_git_protocol_to_https() {
    let result = repository_to_web_url("git://github.com/test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_normalizes_git_plus_git_protocol_to_https() {
    let result = repository_to_web_url("git+git://github.com/test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_strips_fragment_and_query_from_base_url_for_self_hosted() {
    let result = repository_to_web_url("git+https://git.example.com/test/pkg.git#main", None);
    assert_eq!(result.as_deref(), Some("https://git.example.com/test/pkg/tree/main"));
}

#[test]
fn test_falls_back_to_url_parsing_for_self_hosted_git_servers() {
    let result = repository_to_web_url("git+https://git.example.com/test/pkg.git", None);
    assert_eq!(result.as_deref(), Some("https://git.example.com/test/pkg"));
}

#[test]
fn test_returns_none_when_no_repository() {
    let result = pick_repo_url(None);
    assert!(result.is_none());
}

#[test]
fn test_returns_none_for_null_repository() {
    let result = pick_repo_url(Some(&serde_json::Value::Null));
    assert!(result.is_none());
}

#[test]
fn test_throws_when_no_repository_url_defined() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("package.json"), r#"{"name": "test-pkg"}"#).unwrap();
    let result = get_repo_url_from_current_project(dir.path());
    assert!(result.is_err());
}

#[test]
fn test_throws_when_no_package_json_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let result = get_repo_url_from_current_project(dir.path());
    assert!(result.is_err());
}

#[test]
fn test_pick_repo_url_from_object_with_url() {
    let repo = serde_json::json!({"url": "https://github.com/test/pkg"});
    let result = pick_repo_url(Some(&repo));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_pick_repo_url_from_object_with_url_and_directory() {
    let repo =
        serde_json::json!({"url": "https://github.com/test/pkg", "directory": "packages/foo"});
    let result = pick_repo_url(Some(&repo));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/master/packages/foo"));
}

#[test]
fn test_pick_repo_url_from_object_with_url_directory_and_fragment() {
    let repo = serde_json::json!({
        "url": "https://github.com/test/pkg#main",
        "directory": "packages/foo"
    });
    let result = pick_repo_url(Some(&repo));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg/tree/main/packages/foo"));
}

#[test]
fn test_pick_repo_url_from_string() {
    let repo = serde_json::Value::String("test/pkg".to_string());
    let result = pick_repo_url(Some(&repo));
    assert_eq!(result.as_deref(), Some("https://github.com/test/pkg"));
}

#[test]
fn test_repository_to_web_url_empty() {
    assert_eq!(repository_to_web_url("", None), None);
}

#[test]
fn test_redact_url_strips_query_and_fragment() {
    let redacted = redact_url("https://user:pass@github.com/test/pkg?token=secret#frag");
    assert_eq!(redacted, "https://github.com/test/pkg");
}
