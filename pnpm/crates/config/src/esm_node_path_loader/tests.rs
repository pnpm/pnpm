use super::{
    add_esm_node_path_loader_option, esm_node_path_loader_import_flag,
    keep_esm_node_path_loader_option,
};
use pretty_assertions::assert_eq;

/// The TypeScript CLI asserts its derived flag against the same file
/// (`pnpm11/exec/esm-node-path-loader/test/index.ts`), so the two stacks
/// cannot drift apart without one of the tests failing.
#[test]
fn flag_matches_the_golden_copy_shared_with_the_typescript_cli() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../pnpm11/exec/esm-node-path-loader/test/import-flag.txt",
    );
    let golden = std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("the golden flag file must be readable at {path}: {err}");
    });
    assert_eq!(esm_node_path_loader_import_flag(), golden);
}

#[test]
fn flag_never_contains_characters_the_node_options_tokenizer_splits_or_unquotes() {
    let flag = esm_node_path_loader_import_flag();
    assert!(flag.starts_with("--import=data:text/javascript,"));
    assert!(!flag.contains(|char: char| char.is_whitespace() || r#""'\"#.contains(char)));
}

#[test]
fn add_returns_just_the_flag_when_node_options_is_empty() {
    let flag = esm_node_path_loader_import_flag();
    assert_eq!(add_esm_node_path_loader_option(None), flag);
    assert_eq!(add_esm_node_path_loader_option(Some("")), flag);
}

#[test]
fn add_appends_the_flag_without_duplicating_it() {
    let flag = esm_node_path_loader_import_flag();
    let once = add_esm_node_path_loader_option(Some("--max-old-space-size=4096"));
    assert_eq!(once, format!("--max-old-space-size=4096 {flag}"));
    assert_eq!(add_esm_node_path_loader_option(Some(&once)), once);
}

#[test]
fn keep_reapplies_the_flag_only_when_the_previous_value_carried_it() {
    let flag = esm_node_path_loader_import_flag();
    let previous = add_esm_node_path_loader_option(None);
    assert_eq!(
        keep_esm_node_path_loader_option("--no-warnings", Some(&previous)),
        format!("--no-warnings {flag}"),
    );
    assert_eq!(keep_esm_node_path_loader_option("--no-warnings", None), "--no-warnings");
    assert_eq!(
        keep_esm_node_path_loader_option("--no-warnings", Some("--enable-source-maps")),
        "--no-warnings",
    );
}
