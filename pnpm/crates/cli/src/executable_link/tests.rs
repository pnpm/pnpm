use super::replace_executable;
use std::fs;

#[cfg(unix)]
#[test]
fn publishes_the_target_of_a_relative_symlink() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let engine = root.path().join("Cellar/pnpm/bin/pnpm");
    fs::create_dir_all(engine.parent().unwrap()).unwrap();
    fs::write(&engine, b"engine").unwrap();
    fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
    let launcher = root.path().join("bin/pnpm");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("../Cellar/pnpm/bin/pnpm", &launcher).unwrap();
    let dest = root.path().join("global-bin/node");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();

    replace_executable(&launcher, &dest).unwrap();

    assert!(fs::symlink_metadata(&dest).unwrap().is_file());
    assert_eq!(fs::read(&dest).unwrap(), b"engine");
}
