use std::{fs, path::Path};

use super::{is_safe_modules_purge_target, purge_modules_dir_entries};
use pnpm_config::Config;
use tempfile::tempdir;

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

#[test]
fn purge_removes_directory_links_without_following_them() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    let link_target = dir.path().join("link-target");
    fs::create_dir_all(modules_dir.join("plain-dir")).unwrap();
    fs::write(modules_dir.join("plain-file"), "").unwrap();
    fs::create_dir_all(&link_target).unwrap();
    fs::write(link_target.join("package.json"), "{}").unwrap();
    pnpm_fs::symlink_dir(&link_target, &modules_dir.join("linked-dep")).unwrap();

    purge_modules_dir_entries(&modules_dir, &Config::new(), None).unwrap();

    let remaining = fs::read_dir(&modules_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    dbg!(&remaining);
    assert!(remaining.is_empty());
    assert!(link_target.join("package.json").exists(), "the purge must not follow the link");
}
