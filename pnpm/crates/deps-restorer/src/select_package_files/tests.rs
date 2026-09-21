use super::select_package_files;
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

fn index(paths: &[&str]) -> HashMap<String, PathBuf> {
    paths
        .iter()
        .map(|path| (path.to_string(), PathBuf::from(format!("/store/{path}"))))
        .collect()
}

fn names(selected: &HashMap<String, PathBuf>) -> Vec<&str> {
    let mut names: Vec<&str> = selected
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    names
}

fn patterns(patterns: &[&str]) -> Vec<String> {
    patterns
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn no_patterns_import_every_file_and_copy_nothing() {
    let files = index(&["package.json", "index.js", "lib/a.d.ts"]);
    let selected = select_package_files(&files, &[]);
    assert!(matches!(selected, Cow::Borrowed(_)));
    assert_eq!(selected.len(), 3);
}

#[test]
fn patterns_keep_the_files_they_match_and_the_root_manifest() {
    let files = index(&[
        "package.json",
        "index.js",
        "index.d.ts",
        "lib/a.d.ts",
        "lib/package.json",
        "README.md",
    ]);
    let selected = select_package_files(&files, &patterns(&["*.d.ts"]));
    assert_eq!(names(&selected), ["index.d.ts", "lib/a.d.ts", "package.json"]);
}

#[test]
fn a_nested_manifest_is_imported_only_when_a_pattern_names_it() {
    let files = index(&["package.json", "lib/package.json", "lib/a.d.ts"]);
    let selected = select_package_files(&files, &patterns(&["*.d.ts", "*.json"]));
    assert_eq!(names(&selected), ["lib/a.d.ts", "lib/package.json", "package.json"]);
}

#[test]
fn an_excluding_pattern_takes_files_back_out() {
    let files = index(&["package.json", "a.ts", "a.test.ts"]);
    let selected = select_package_files(&files, &patterns(&["*.ts", "!*.test.ts"]));
    assert_eq!(names(&selected), ["a.ts", "package.json"]);
}
