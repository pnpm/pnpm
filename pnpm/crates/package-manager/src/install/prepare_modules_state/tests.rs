use std::path::Path;

use super::is_safe_modules_purge_target;

#[test]
fn modules_purge_target_must_be_a_strict_workspace_descendant() {
    let workspace_root = Path::new("/workspace");
    let modules_dir = Path::new("/workspace/node_modules");

    assert!(!is_safe_modules_purge_target(workspace_root, workspace_root));
    assert!(is_safe_modules_purge_target(modules_dir, workspace_root));
    assert!(!is_safe_modules_purge_target(
        Path::new("/workspace-sibling/node_modules"),
        workspace_root,
    ));
}
