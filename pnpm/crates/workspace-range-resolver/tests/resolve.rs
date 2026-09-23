use pnpm_workspace_range_resolver::resolve_workspace_range;

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

fn owned(versions: &[&str]) -> Vec<String> {
    versions
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn resolves_wildcards_to_a_non_semver_version_when_no_semver_version_is_present() {
    assert_eq!(resolve_workspace_range("*", &owned(&["1"])).as_deref(), Some("1"));
    assert_eq!(resolve_workspace_range("^", &owned(&["1.0"])).as_deref(), Some("1.0"));
    assert_eq!(resolve_workspace_range("~", &owned(&["1"])).as_deref(), Some("1"));
    assert_eq!(resolve_workspace_range("", &owned(&["1"])).as_deref(), Some("1"));
    assert_eq!(resolve_workspace_range("*", &owned(&["1", "2"])).as_deref(), Some("2"));
    assert_eq!(resolve_workspace_range("*", &owned(&["1", "1.0.0"])).as_deref(), Some("1.0.0"));
    assert_eq!(
        resolve_workspace_range("*", &owned(&["\u{10000}", "\u{E000}"])).as_deref(),
        Some("\u{E000}"),
    );
}

#[test]
fn resolves_a_range_identical_to_a_non_semver_version() {
    assert_eq!(resolve_workspace_range("1", &owned(&["1"])).as_deref(), Some("1"));
    assert_eq!(resolve_workspace_range("1.0", &owned(&["1.0", "2"])).as_deref(), Some("1.0"));
    assert_eq!(resolve_workspace_range("2", &owned(&["1"])), None);
}
