use super::{WorkspaceSettings, assert_eq};

/// An empty `scope` is a value like any other — it would clear a scope the
/// global `config.yaml` set — so it is refused like a non-empty one, while a
/// file that names no scope reports nothing.
#[test]
fn an_empty_scope_is_refused_and_a_missing_one_is_not_reported() {
    let mut empty = WorkspaceSettings::default();
    empty.collect_key_issues("scope: ''\n");
    assert_eq!(empty.key_issues.refused, vec!["scope".to_owned()]);

    let mut absent = WorkspaceSettings::default();
    absent.collect_key_issues("registry: https://reg.example/\n");
    assert!(absent.key_issues.is_empty());
}
