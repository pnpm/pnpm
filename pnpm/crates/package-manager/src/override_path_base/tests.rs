use super::local_path_base;
use std::path::{Path, PathBuf};

#[test]
fn a_pinned_lockfile_directory_takes_precedence() {
    let config = pnpm_config::Config {
        lockfile_dir: Some(PathBuf::from("locks")),
        workspace_dir: Some(PathBuf::from("workspace")),
        ..pnpm_config::Config::default()
    };

    assert_eq!(local_path_base(&config, Path::new("project")), Path::new("locks"));
}

#[test]
fn workspace_directory_is_used_without_a_pinned_lockfile_directory() {
    let config = pnpm_config::Config {
        workspace_dir: Some(PathBuf::from("workspace")),
        ..pnpm_config::Config::default()
    };

    assert_eq!(local_path_base(&config, Path::new("project")), Path::new("workspace"));
}

#[test]
fn lockfile_directory_is_the_fallback_outside_a_workspace() {
    let config = pnpm_config::Config::default();

    assert_eq!(local_path_base(&config, Path::new("project")), Path::new("project"));
}
