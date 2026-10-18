use super::target_swap_lock_path;
use std::fs;

#[test]
fn target_swap_lock_paths_resolve_directory_aliases() {
    let temp_dir = tempfile::tempdir().unwrap();
    let real_root = temp_dir.path().join("real");
    let alias_root = temp_dir.path().join("alias");
    fs::create_dir(&real_root).unwrap();
    pnpm_fs::symlink_dir(&real_root, &alias_root).unwrap();

    let real_target = target_swap_lock_path(&real_root.join("package")).unwrap();
    let alias_target = target_swap_lock_path(&alias_root.join("package")).unwrap();

    assert_eq!(real_target, alias_target);
}
