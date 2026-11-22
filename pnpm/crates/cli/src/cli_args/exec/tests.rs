use super::{
    ExecDirs,
    command_search_path,
    configured_node_options,
};
use pnpm_config::Config;
use std::path::{
    Path,
    PathBuf,
};

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
    config.extra_bin_paths = vec![extra_bin.clone()];
    let project = Path::new("/workspace/project");
    let dirs = ExecDirs::same(project);
    let path = command_search_path(dirs, &config, None).expect("search path");
    let entries: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| *entry == &extra_bin)
            .count(),
        1,
    );
}

#[test]
fn command_search_path_uses_the_project_environment_without_a_workspace() {
    let mut config = Config::default();
    config.python.enabled = true;
    let project = Path::new("/app");
    let run = project.join("src");
    let path = command_search_path(ExecDirs { project, run: &run }, &config, None).unwrap();
    let bin = if cfg!(windows) { "Scripts" } else { "bin" };
    assert!(std::env::split_paths(&path).any(|entry| entry == project.join(".venv").join(bin)));
}
