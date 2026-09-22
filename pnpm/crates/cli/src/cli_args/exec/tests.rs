use super::{ExecDirs, command_search_path, configured_node_options};
use pnpm_config::Config;
use std::path::{Path, PathBuf};

#[test]
fn configured_node_options_preserves_extra_env_without_a_node_options_setting() {
    let mut config = Config::default();
    config.extra_env.insert("NODE_OPTIONS".to_string(), "--trace-warnings".to_string());

    assert_eq!(configured_node_options(&config).as_deref(), Some("--trace-warnings"));
}

#[test]
fn command_search_path_includes_extra_bin_paths() {
    let mut config = Config::default();
    let extra_bin = PathBuf::from("/custom/extra/bin");
    config.extra_bin_paths = vec![extra_bin];
    let project = Path::new("/workspace/project");
    let dirs = ExecDirs::same(project);
    let path = command_search_path(dirs, &config, None).expect("search path");
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("/custom/extra/bin"));
}
