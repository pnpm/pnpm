use super::{
    is_windows_drive_path, replacement_aliases, should_replace_existing_package,
    split_comma_separated, update_selectors,
};
use pacquet_global::GlobalPackageInfo;
use std::path::{Path, PathBuf};

#[test]
fn comma_splits_into_selectors() {
    let base = Path::new("/nonexistent");
    assert_eq!(split_comma_separated("foo,bar", base), vec!["foo", "bar"]);
    assert_eq!(split_comma_separated("foo", base), vec!["foo"]);
}

#[test]
fn urls_are_kept_whole() {
    let base = Path::new("/nonexistent");
    assert_eq!(
        split_comma_separated("https://example.com/a,b.tgz", base),
        vec!["https://example.com/a,b.tgz"],
    );
}

#[test]
fn detects_windows_drive_paths() {
    assert!(is_windows_drive_path(r"C:\foo"));
    assert!(is_windows_drive_path("d:/bar"));
    assert!(!is_windows_drive_path("foo"));
}

#[test]
fn latest_update_queries_the_registry_only_for_registry_packages() {
    let dependencies = vec![
        ("private-linked-pkg".to_string(), "link:/home/user/private-linked-pkg".to_string()),
        ("local-tarball-pkg".to_string(), "file:/home/user/local-tarball-pkg.tgz".to_string()),
        ("foo".to_string(), "^1.0.0".to_string()),
    ];
    assert_eq!(
        update_selectors(&dependencies, true),
        vec![
            "private-linked-pkg@link:/home/user/private-linked-pkg",
            "local-tarball-pkg@file:/home/user/local-tarball-pkg.tgz",
            "foo",
        ],
    );
    assert_eq!(
        update_selectors(&dependencies, false),
        vec![
            "private-linked-pkg@link:/home/user/private-linked-pkg",
            "local-tarball-pkg@file:/home/user/local-tarball-pkg.tgz",
            "foo@^1.0.0",
        ],
    );
}

#[test]
fn pnpm_package_aliases_replace_each_other() {
    assert_eq!(replacement_aliases(&["@pnpm/exe".to_string()]), vec!["@pnpm/exe", "pnpm"]);
    assert_eq!(replacement_aliases(&["pnpm".to_string()]), vec!["pnpm", "@pnpm/exe"]);
}

#[test]
fn unrelated_aliases_are_not_expanded() {
    assert_eq!(
        replacement_aliases(&["eslint".to_string(), "typescript".to_string()]),
        vec!["eslint", "typescript"],
    );
}

#[test]
fn pnpm_alias_equivalence_only_replaces_pnpm_cli_groups() {
    let aliases = vec!["@pnpm/exe".to_string()];
    let aliases_to_replace = replacement_aliases(&aliases);

    assert!(should_replace_existing_package(
        &global_package(&["pnpm"]),
        &aliases,
        &aliases_to_replace,
    ));
    assert!(!should_replace_existing_package(
        &global_package(&["pnpm", "eslint"]),
        &aliases,
        &aliases_to_replace,
    ));
}

#[test]
fn exact_aliases_still_replace_mixed_groups() {
    let aliases = vec!["@pnpm/exe".to_string()];
    let aliases_to_replace = replacement_aliases(&aliases);

    assert!(should_replace_existing_package(
        &global_package(&["@pnpm/exe", "eslint"]),
        &aliases,
        &aliases_to_replace,
    ));
}

fn global_package(aliases: &[&str]) -> GlobalPackageInfo {
    GlobalPackageInfo {
        hash: "hash".to_string(),
        install_dir: PathBuf::from("/global/hash"),
        dependencies: aliases
            .iter()
            .map(|alias| ((*alias).to_string(), "1.0.0".to_string()))
            .collect(),
    }
}
