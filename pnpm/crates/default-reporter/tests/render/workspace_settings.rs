use super::{CWD, render, scope, scope_reporting_state};

#[test]
fn reports_an_unnarrowed_workspace_scope() {
    let mut reporter = scope_reporting_state();
    assert_eq!(
        render(&mut reporter, vec![scope(3, Some(3), Some(CWD))]),
        "Scope: all 3 workspace projects",
    );
}

#[test]
fn reports_a_narrowed_workspace_scope() {
    let mut reporter = scope_reporting_state();
    assert_eq!(
        render(&mut reporter, vec![scope(2, Some(3), Some(CWD))]),
        "Scope: 2 of 3 workspace projects",
    );
}

/// Outside a workspace there are no "workspace projects" to count, which
/// is the shape pnpm renders without the qualifier.
#[test]
fn reports_a_scope_without_a_workspace_prefix_as_plain_projects() {
    let mut reporter = scope_reporting_state();
    assert_eq!(render(&mut reporter, vec![scope(2, None, None)]), "Scope: 2 projects");
}
