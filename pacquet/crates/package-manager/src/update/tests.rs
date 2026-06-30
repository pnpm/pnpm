use super::{is_workspace_local_path_specifier, parse_update_param};

#[test]
fn parses_bare_name_without_version() {
    let parsed = parse_update_param("foo");
    assert_eq!(parsed.pattern, "foo");
    assert_eq!(parsed.version, None);
}

#[test]
fn parses_name_with_version() {
    let parsed = parse_update_param("foo@2");
    assert_eq!(parsed.pattern, "foo");
    assert_eq!(parsed.version.as_deref(), Some("2"));
}

#[test]
fn leading_scope_at_is_not_a_version_separator() {
    let parsed = parse_update_param("@scope/foo");
    assert_eq!(parsed.pattern, "@scope/foo");
    assert_eq!(parsed.version, None);
}

#[test]
fn scoped_name_with_version_splits_on_last_at() {
    let parsed = parse_update_param("@scope/foo@^1.2.3");
    assert_eq!(parsed.pattern, "@scope/foo");
    assert_eq!(parsed.version.as_deref(), Some("^1.2.3"));
}

#[test]
fn wildcard_pattern_without_version() {
    let parsed = parse_update_param("@pnpm.e2e/peer-*");
    assert_eq!(parsed.pattern, "@pnpm.e2e/peer-*");
    assert_eq!(parsed.version, None);
}

#[test]
fn negated_scoped_pattern_is_not_split_on_scope_at() {
    let parsed = parse_update_param("!@pnpm.e2e/peer-*");
    assert_eq!(parsed.pattern, "!@pnpm.e2e/peer-*");
    assert_eq!(parsed.version, None);
}

#[test]
fn negated_unscoped_pattern_without_version() {
    let parsed = parse_update_param("!foo");
    assert_eq!(parsed.pattern, "!foo");
    assert_eq!(parsed.version, None);
}

#[test]
fn workspace_local_path_specifiers_are_detected() {
    for spec in [
        "workspace:.",
        "workspace:./packages/foo",
        "workspace:../packages/foo/dist",
        "workspace:/abs/path",
        "workspace:~/home/path",
        r"workspace:C:\packages\foo",
    ] {
        assert!(is_workspace_local_path_specifier(spec), "expected {spec} to be a local path");
    }
}

#[test]
fn workspace_range_specifiers_are_not_local_paths() {
    for spec in [
        "workspace:*",
        "workspace:^",
        "workspace:~",
        "workspace:^1.0.0",
        "workspace:~1.2.3",
        "workspace:1.0.0",
        "workspace:alias@*",
        "^1.0.0",
        "link:../foo",
    ] {
        assert!(!is_workspace_local_path_specifier(spec), "expected {spec} not to be a local path");
    }
}
