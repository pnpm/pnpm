use serde_json::json;

use super::{
    is_http_url, parse_package_spec, pick_bugs_url, repository_to_issues_url,
    try_hosted_git_shorthand,
};

#[test]
fn pick_bugs_url_returns_bugs_url_from_object() {
    let manifest = json!({
        "bugs": { "url": "https://github.com/test/pkg/issues" }
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/test/pkg/issues"));
}

#[test]
fn pick_bugs_url_returns_bugs_url_from_string() {
    let manifest = json!({
        "bugs": "https://github.com/test/pkg/issues"
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/test/pkg/issues"));
}

#[test]
fn pick_bugs_url_falls_back_to_repository_issues_url() {
    let manifest = json!({
        "repository": "https://github.com/test/pkg"
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/test/pkg/issues"));
}

#[test]
fn pick_bugs_url_prefers_bugs_over_repository() {
    let manifest = json!({
        "bugs": { "url": "https://github.com/other/issues" },
        "repository": "https://github.com/test/pkg"
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/other/issues"));
}

#[test]
fn pick_bugs_url_returns_none_when_no_bugs_url() {
    let manifest = json!({ "name": "test-pkg" });
    assert_eq!(pick_bugs_url(&manifest), None);
}

#[test]
fn pick_bugs_url_returns_none_for_non_http_bugs() {
    let manifest = json!({
        "bugs": "ftp://example.com/bugs"
    });
    assert_eq!(pick_bugs_url(&manifest), None);
}

#[test]
fn pick_bugs_url_returns_none_for_empty_bugs() {
    let manifest = json!({
        "bugs": {}
    });
    assert_eq!(pick_bugs_url(&manifest), None);
}

#[test]
fn repo_url_normalizes_git_https_with_dot_git() {
    assert_eq!(
        repository_to_issues_url("git+https://github.com/test/pkg.git"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_trailing_slash() {
    assert_eq!(
        repository_to_issues_url("https://github.com/test/pkg/"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_shorthand_owner_repo() {
    assert_eq!(
        repository_to_issues_url("test/pkg"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_github_shorthand() {
    assert_eq!(
        repository_to_issues_url("github:test/pkg"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_git_ssh_url() {
    assert_eq!(
        repository_to_issues_url("git+ssh://git@github.com/test/pkg.git"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_gitlab_shorthand() {
    assert_eq!(
        repository_to_issues_url("gitlab:test/pkg"),
        Some("https://gitlab.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_falls_back_for_self_hosted_git_server() {
    assert_eq!(
        repository_to_issues_url("git+https://git.example.com/test/pkg.git"),
        Some("https://git.example.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_handles_dot_git_with_trailing_slash() {
    assert_eq!(
        repository_to_issues_url("git+https://github.com/test/pkg.git/"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_fragment_and_query() {
    assert_eq!(
        repository_to_issues_url("git+https://github.com/test/pkg.git#main"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_scp_style_ssh() {
    assert_eq!(
        repository_to_issues_url("git@github.com:test/pkg.git"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_bitbucket_shorthand() {
    assert_eq!(
        repository_to_issues_url("bitbucket:test/pkg"),
        Some("https://bitbucket.org/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_returns_none_for_invalid_url() {
    assert_eq!(repository_to_issues_url("not-a-url"), None);
}

#[test]
fn is_http_url_accepts_https() {
    assert!(is_http_url("https://example.com"));
}

#[test]
fn is_http_url_accepts_http() {
    assert!(is_http_url("http://example.com"));
}

#[test]
fn is_http_url_rejects_ftp() {
    assert!(!is_http_url("ftp://example.com"));
}

#[test]
fn is_http_url_rejects_invalid() {
    assert!(!is_http_url("not-a-url"));
}

#[test]
fn hosted_shorthand_recognises_github_prefix() {
    assert_eq!(
        try_hosted_git_shorthand("github:owner/repo"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_recognises_gitlab_prefix() {
    assert_eq!(
        try_hosted_git_shorthand("gitlab:owner/repo"),
        Some("https://gitlab.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_recognises_bitbucket_prefix() {
    assert_eq!(
        try_hosted_git_shorthand("bitbucket:owner/repo"),
        Some("https://bitbucket.org/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_recognises_bare_owner_repo() {
    assert_eq!(
        try_hosted_git_shorthand("owner/repo"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_returns_none_for_urls() {
    assert_eq!(try_hosted_git_shorthand("https://github.com/owner/repo"), None);
}

#[test]
fn hosted_shorthand_returns_none_for_git_urls() {
    assert_eq!(try_hosted_git_shorthand("git@github.com:owner/repo.git"), None);
}

#[test]
fn parse_spec_bare_name() {
    assert_eq!(parse_package_spec("foo"), ("foo", None));
}

#[test]
fn parse_spec_name_with_version() {
    assert_eq!(parse_package_spec("foo@1.0.0"), ("foo", Some("1.0.0")));
}

#[test]
fn parse_spec_scoped_package() {
    assert_eq!(parse_package_spec("@scope/foo"), ("@scope/foo", None));
}

#[test]
fn parse_spec_scoped_with_version() {
    assert_eq!(parse_package_spec("@scope/foo@1.0.0"), ("@scope/foo", Some("1.0.0")));
}

#[test]
fn parse_spec_scoped_with_tag() {
    assert_eq!(parse_package_spec("@scope/foo@latest"), ("@scope/foo", Some("latest")));
}

#[test]
fn parse_spec_trims_whitespace() {
    assert_eq!(parse_package_spec("  foo  "), ("foo", None));
}

#[test]
fn parse_spec_strips_version_from_name_with_version() {
    let (name, tag) = parse_package_spec("react@18.2.0");
    assert_eq!(name, "react");
    assert_eq!(tag, Some("18.2.0"));
}

#[test]
fn repo_url_resolves_github_dot_com_shorthand_without_scheme() {
    assert_eq!(
        repository_to_issues_url("github.com/owner/repo"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn repo_url_retains_ssh_port() {
    assert_eq!(
        repository_to_issues_url("ssh://git@git.example.com:2222/owner/repo.git"),
        Some("https://git.example.com:2222/owner/repo/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_shorthand_fragment_and_query() {
    assert_eq!(
        repository_to_issues_url("github:owner/repo#main"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
    assert_eq!(
        repository_to_issues_url("owner/repo.git#main"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_scp_fragment_and_query() {
    assert_eq!(
        repository_to_issues_url("git@github.com:owner/repo.git#main"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}
