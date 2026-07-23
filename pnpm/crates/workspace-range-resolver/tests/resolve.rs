use pacquet_workspace_range_resolver::resolve_workspace_range;

fn versions() -> Vec<String> {
    vec!["1.0.0".to_string(), "2.0.0".to_string(), "3.0.0-beta.1".to_string()]
}

#[test]
fn resolves_star_to_max_version_including_prereleases() {
    assert_eq!(resolve_workspace_range("*", &versions()).as_deref(), Some("3.0.0-beta.1"));
}

#[test]
fn resolves_caret_to_max_version_including_prereleases() {
    assert_eq!(resolve_workspace_range("^", &versions()).as_deref(), Some("3.0.0-beta.1"));
}

#[test]
fn resolves_tilde_to_max_version_including_prereleases() {
    assert_eq!(resolve_workspace_range("~", &versions()).as_deref(), Some("3.0.0-beta.1"));
}

#[test]
fn resolves_empty_string_to_max_version_including_prereleases() {
    assert_eq!(resolve_workspace_range("", &versions()).as_deref(), Some("3.0.0-beta.1"));
}

#[test]
fn resolves_semver_range() {
    assert_eq!(resolve_workspace_range("^1.0.0", &versions()).as_deref(), Some("1.0.0"));
    assert_eq!(resolve_workspace_range("^2.0.0", &versions()).as_deref(), Some("2.0.0"));
    assert_eq!(resolve_workspace_range(">=1.0.0", &versions()).as_deref(), Some("2.0.0"));
}

#[test]
fn returns_none_when_no_version_satisfies_range() {
    assert_eq!(resolve_workspace_range("^4.0.0", &versions()), None);
}
