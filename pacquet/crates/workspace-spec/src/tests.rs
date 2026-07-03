use super::WorkspaceSpec;

fn ws(version: impl Into<String>, alias: Option<&str>) -> WorkspaceSpec {
    WorkspaceSpec { alias: alias.map(str::to_string), version: version.into() }
}

#[test]
fn parse_valid_workspace_spec() {
    assert_eq!(WorkspaceSpec::parse("workspace:*"), Some(ws("*", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:^"), Some(ws("^", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:~"), Some(ws("~", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:0.1.2"), Some(ws("0.1.2", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:foo@*"), Some(ws("*", Some("foo"))));
    assert_eq!(WorkspaceSpec::parse("workspace:foo@^"), Some(ws("^", Some("foo"))));
    assert_eq!(WorkspaceSpec::parse("workspace:foo@~"), Some(ws("~", Some("foo"))));
    assert_eq!(WorkspaceSpec::parse("workspace:foo@0.1.2"), Some(ws("0.1.2", Some("foo"))));
    assert_eq!(WorkspaceSpec::parse("workspace:@foo/bar@*"), Some(ws("*", Some("@foo/bar"))));
    assert_eq!(WorkspaceSpec::parse("workspace:@foo/bar@^"), Some(ws("^", Some("@foo/bar"))));
    assert_eq!(WorkspaceSpec::parse("workspace:@foo/bar@~"), Some(ws("~", Some("@foo/bar"))));
    assert_eq!(
        WorkspaceSpec::parse("workspace:@foo/bar@0.1.2"),
        Some(ws("0.1.2", Some("@foo/bar"))),
    );
}

#[test]
fn parse_invalid_workspace_spec() {
    assert_eq!(WorkspaceSpec::parse("npm:foo@0.1.2"), None);
    assert_eq!(WorkspaceSpec::parse("*"), None);
}

#[test]
fn to_string_round_trips() {
    assert_eq!(ws("*", None).to_string(), "workspace:*");
    assert_eq!(ws("^", None).to_string(), "workspace:^");
    assert_eq!(ws("~", None).to_string(), "workspace:~");
    assert_eq!(ws("0.1.2", None).to_string(), "workspace:0.1.2");
    assert_eq!(ws("*", Some("foo")).to_string(), "workspace:foo@*");
    assert_eq!(ws("^", Some("foo")).to_string(), "workspace:foo@^");
    assert_eq!(ws("~", Some("foo")).to_string(), "workspace:foo@~");
    assert_eq!(ws("0.1.2", Some("foo")).to_string(), "workspace:foo@0.1.2");
    assert_eq!(ws("*", Some("@foo/bar")).to_string(), "workspace:@foo/bar@*");
    assert_eq!(ws("^", Some("@foo/bar")).to_string(), "workspace:@foo/bar@^");
    assert_eq!(ws("~", Some("@foo/bar")).to_string(), "workspace:@foo/bar@~");
    assert_eq!(ws("0.1.2", Some("@foo/bar")).to_string(), "workspace:@foo/bar@0.1.2");
}

/// The fields are plain (no setters / phantom state), so mutating
/// `alias` and `version` directly re-renders the spec correctly.
#[test]
fn mutate_alias_and_version() {
    let mut spec = WorkspaceSpec::parse("workspace:*").expect("parses");
    assert_eq!(spec.to_string(), "workspace:*");
    spec.version = "^".to_string();
    assert_eq!(spec.to_string(), "workspace:^");
    spec.alias = Some("foo".to_string());
    assert_eq!(spec.to_string(), "workspace:foo@^");
    spec.alias = None;
    assert_eq!(spec.to_string(), "workspace:^");
}

#[test]
fn empty_version_is_preserved() {
    assert_eq!(WorkspaceSpec::parse("workspace:"), Some(ws("", None)));
    assert_eq!(
        WorkspaceSpec::parse("workspace:").map(|spec| spec.to_string()).as_deref(),
        Some("workspace:"),
    );
}

#[test]
fn alias_first_char_class_excludes_dot_underscore_slash() {
    assert_eq!(WorkspaceSpec::parse("workspace:./foo"), Some(ws("./foo", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:../foo"), Some(ws("../foo", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:_foo@1.0.0"), Some(ws("_foo@1.0.0", None)));
    assert_eq!(WorkspaceSpec::parse("workspace:/abs/path"), Some(ws("/abs/path", None)));
}
