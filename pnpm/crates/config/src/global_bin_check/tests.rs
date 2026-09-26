use super::{CheckGlobalBinDirError, check_global_bin_dir, has_unexpanded_env_reference};
use std::path::{Path, PathBuf};

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
    let other = tmp
        .path()
        .join("other")
        .to_string_lossy()
        .into_owned();
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

#[test]
fn not_in_path_suggests_setup_when_no_entry_is_unexpanded() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let path_env = tmp
        .path()
        .join("other")
        .to_string_lossy()
        .into_owned();
    let Err(CheckGlobalBinDirError::NotInPath { hint, .. }) =
        check_global_bin_dir(&bin, Some(&path_env), false)
    else {
        panic!("expected NotInPath");
    };
    assert_eq!(hint, r#"Run "pnpm setup" to update your shell configuration."#);
}

/// Windows leaves `%PNPM_HOME%` in the user `Path` verbatim when
/// `PNPM_HOME` is a `REG_EXPAND_SZ` user variable (pnpm/pnpm#5283).
#[test]
fn not_in_path_names_an_unexpanded_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let path_env = std::env::join_paths([tmp.path().join("other"), PathBuf::from("%PNPM_HOME%")])
        .unwrap()
        .into_string()
        .unwrap();
    let Err(CheckGlobalBinDirError::NotInPath { hint, .. }) =
        check_global_bin_dir(&bin, Some(&path_env), false)
    else {
        panic!("expected NotInPath");
    };
    assert_eq!(
        hint,
        r#"PATH contains "%PNPM_HOME%", which was not expanded. On Windows, a variable referenced from the user Path must be set and stored as a plain string (REG_SZ), not an expandable string (REG_EXPAND_SZ). Fix the variable, then open a new terminal."#,
    );
}

#[test]
fn detects_env_references() {
    for dir in ["%PNPM_HOME%", r"%PNPM_HOME%\bin", r"C:\%A%\b", "%%%A%"] {
        assert!(has_unexpanded_env_reference(dir), "{dir}");
    }
    for dir in [r"C:\bin", "100%", "%", "%%", r"C:\50%\b", "a%%b%c"] {
        assert!(!has_unexpanded_env_reference(dir), "{dir}");
    }
}
