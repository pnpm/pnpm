use super::{is_windows_drive_path, replacement_aliases, split_comma_separated};
use std::path::Path;

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
