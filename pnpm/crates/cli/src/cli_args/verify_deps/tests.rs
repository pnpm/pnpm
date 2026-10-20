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
