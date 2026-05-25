use super::{MakeEnv, make_env};
use std::path::{Path, PathBuf};

#[test]
fn make_env_prepends_bin_dir_and_stamps_user_agent() {
    let dir = Path::new("/tmp/project-xyz");
    let env = make_env(MakeEnv {
        dir,
        extra_bin_paths: &[],
        node_options: None,
        package_name: Some("my-pkg"),
        user_agent: "pnpm",
    });

    let path = env
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
        .map(|(_, v)| v.clone())
        .expect("PATH must be present");
    let expected_bin = dir.join("node_modules").join(".bin");
    let first = std::env::split_paths(&path).next().expect("PATH has at least one entry");
    assert_eq!(first, expected_bin, "node_modules/.bin must be first on PATH");

    assert_eq!(env.get("npm_config_user_agent").map(String::as_str), Some("pnpm"));
    assert_eq!(env.get("PNPM_PACKAGE_NAME").map(String::as_str), Some("my-pkg"));
}

#[test]
fn make_env_includes_extra_bin_paths_and_node_options() {
    let dir = Path::new("/tmp/project-xyz");
    let extra = vec![PathBuf::from("/opt/tools/bin")];
    let env = make_env(MakeEnv {
        dir,
        extra_bin_paths: &extra,
        node_options: Some("--max-old-space-size=4096"),
        package_name: None,
        user_agent: "pnpm",
    });

    let path = env
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
        .map(|(_, v)| v.clone())
        .expect("PATH present");
    let entries: Vec<PathBuf> = std::env::split_paths(&path).collect();
    assert_eq!(entries[0], dir.join("node_modules").join(".bin"));
    assert_eq!(entries[1], PathBuf::from("/opt/tools/bin"));

    assert_eq!(env.get("NODE_OPTIONS").map(String::as_str), Some("--max-old-space-size=4096"));
    assert!(!env.contains_key("PNPM_PACKAGE_NAME"), "no package name -> key absent");
}
