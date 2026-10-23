use super::{super::super::InstallError, refuse_drifted_modules_dir, same_path};
use pnpm_config::{Config, NodeLinker};
use std::path::Path;

#[test]
fn same_path_compares_lexically() {
    assert!(same_path(Path::new("/a/store/../store/v11"), Path::new("/a/store/v11")));
    assert!(!same_path(Path::new("/a/store/v11"), Path::new("/a/other/v11")));
}

#[cfg(windows)]
#[test]
fn same_path_ignores_non_ascii_case_on_windows() {
    assert!(same_path(Path::new(r"C:\Störe\v11"), Path::new(r"c:\STÖRE\v11")));
    assert!(!same_path(Path::new(r"C:\Störe\v11"), Path::new(r"C:\Store\v11")));
}

#[test]
fn an_unreadable_modules_manifest_is_refused() {
    let result = refuse_drifted_modules_dir(None, &Config::new(), NodeLinker::Isolated);
    assert!(matches!(result, Err(InstallError::ModulesBreakingChange { .. })), "{result:?}");
}
