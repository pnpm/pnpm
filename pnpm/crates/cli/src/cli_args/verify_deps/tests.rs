use super::{filter_selector_args, install_config_args};
use pnpm_config::Config;
use std::path::PathBuf;

fn selectors(selectors: &[&str]) -> Vec<String> {
    selectors
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn filter_selector_args_forward_the_selection_with_dependencies() {
    assert!(filter_selector_args(&[], &[]).is_empty());
    assert_eq!(
        filter_selector_args(&selectors(&["foo", "bar"]), &[]),
        ["--filter=foo...", "--filter=bar..."],
    );
    assert_eq!(filter_selector_args(&[], &selectors(&["foo"])), ["--filter-prod=foo..."]);
}

#[test]
fn filter_selector_args_keep_exclusions_and_dependency_selectors() {
    assert_eq!(
        filter_selector_args(&selectors(&["foo...", "...bar", "baz^...", "!qux"]), &[]),
        ["--filter=foo...", "--filter=...bar...", "--filter=baz^...", "--filter=!qux"],
    );
}

#[test]
fn install_config_args_forward_the_selection_patterns_the_command_line_set() {
    let mut config = Config {
        test_pattern: vec!["test/**".to_string(), "**/*.spec.ts".to_string()],
        changed_files_ignore_pattern: vec!["**/*.md".to_string()],
        ..Default::default()
    };
    assert_eq!(install_config_args(&config).count(), 0);

    config.cli_settings.insert("testPattern".to_string());
    config.cli_settings.insert("changedFilesIgnorePattern".to_string());
    assert_eq!(
        install_config_args(&config).collect::<Vec<_>>(),
        [
            "--test-pattern=test/**",
            "--test-pattern=**/*.spec.ts",
            "--changed-files-ignore-pattern=**/*.md",
        ],
    );
}

#[test]
fn install_config_args_resolves_relative_npmrc_auth_file_against_current_dir() {
    let config =
        Config { npmrc_auth_file: Some(PathBuf::from("custom-user.npmrc")), ..Default::default() };
    let expected = std::env::current_dir().expect("read current dir").join("custom-user.npmrc");
    assert_eq!(
        install_config_args(&config).collect::<Vec<_>>(),
        [format!("--npmrc-auth-file={}", expected.display())],
    );

    let abs_path = std::env::current_dir().expect("read current dir").join("abs-user.npmrc");
    let abs_config = Config { npmrc_auth_file: Some(abs_path.clone()), ..Default::default() };
    assert_eq!(
        install_config_args(&abs_config).collect::<Vec<_>>(),
        [format!("--npmrc-auth-file={}", abs_path.display())],
    );
}
