use super::lock::target_swap_lock_path;
use std::fs;

#[test]
fn target_swap_lock_paths_resolve_directory_aliases() {
    let temp_dir = tempfile::tempdir().unwrap();
    let real_root = temp_dir.path().join("real");
    let alias_root = temp_dir.path().join("alias");
    let real_target = real_root.join("package");
    let symlink_target = real_root.join("package-link");
    fs::create_dir(&real_root).unwrap();
    fs::create_dir(&real_target).unwrap();
    pnpm_fs::symlink_dir(&real_root, &alias_root).unwrap();
    pnpm_fs::symlink_dir(&real_target, &symlink_target).unwrap();

    let real_lock_path = target_swap_lock_path(&real_target).unwrap();
    let alias_lock_path = target_swap_lock_path(&alias_root.join("package")).unwrap();
    let symlink_lock_path = target_swap_lock_path(&symlink_target).unwrap();

    assert_eq!(real_lock_path, alias_lock_path);
    assert_ne!(real_lock_path, symlink_lock_path);
}
