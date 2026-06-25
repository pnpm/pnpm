use super::{CheckGlobalBinDirError, check_global_bin_dir};
use std::path::Path;

#[test]
fn no_path_env_when_unset_or_empty() {
    let dir = Path::new("/some/bin");
    assert!(matches!(
        check_global_bin_dir(dir, None, false),
        Err(CheckGlobalBinDirError::NoPathEnv)
    ));
    assert!(matches!(
        check_global_bin_dir(dir, Some(""), false),
        Err(CheckGlobalBinDirError::NoPathEnv)
    ));
}

#[test]
fn not_in_path_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let other = tmp.path().join("other").to_string_lossy().into_owned();
    let result = check_global_bin_dir(&bin, Some(&other), false);
    assert!(matches!(result, Err(CheckGlobalBinDirError::NotInPath { .. })));
}

#[test]
fn ok_when_in_path() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path_env = bin.to_string_lossy().into_owned();
    check_global_bin_dir(&bin, Some(&path_env), true).unwrap();
}
