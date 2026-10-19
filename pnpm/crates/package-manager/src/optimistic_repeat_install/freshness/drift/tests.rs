use super::importer_depends_on_unproven_project;
use std::path::{Path, PathBuf};

#[test]
fn unproven_workspace_root_does_not_mark_unrelated_child_project() {
    let workspace_root = Path::new("/ws");
    let root_manifest = serde_json::json!({ "name": "root", "private": true });
    let b_manifest = serde_json::json!({ "name": "b", "version": "1.0.0" });
    let importer_a: pnpm_lockfile::ProjectSnapshot = serde_saphyr::from_str(
        r"
dependencies:
  b:
    specifier: workspace:*
    version: file:packages/b
",
    )
    .expect("parse importer_a");

    let unproven_root = vec![(PathBuf::from("/ws"), &root_manifest)];
    assert!(
        !importer_depends_on_unproven_project(
            workspace_root,
            Path::new("/ws/packages/a"),
            &importer_a,
            &unproven_root,
        ),
        "unproven workspace root `/ws` must not match `file:packages/b` via prefix",
    );

    let unproven_b = vec![(PathBuf::from("/ws/packages/b"), &b_manifest)];
    assert!(
        importer_depends_on_unproven_project(
            workspace_root,
            Path::new("/ws/packages/a"),
            &importer_a,
            &unproven_b,
        ),
        "unproven `packages/b` must match `file:packages/b`",
    );
}

#[test]
fn link_dependency_on_unproven_project_is_detected() {
    let workspace_root = Path::new("/ws");
    let b_manifest = serde_json::json!({ "name": "b", "version": "2.0.0" });
    let importer_a: pnpm_lockfile::ProjectSnapshot = serde_saphyr::from_str(
        r"
dependencies:
  b:
    specifier: workspace:^1.0.0
    version: link:../b
",
    )
    .expect("parse importer_a");

    let unproven_b = vec![(PathBuf::from("/ws/packages/b"), &b_manifest)];
    assert!(
        importer_depends_on_unproven_project(
            workspace_root,
            Path::new("/ws/packages/a"),
            &importer_a,
            &unproven_b,
        ),
        "link:../b from /ws/packages/a resolves to /ws/packages/b",
    );
}
