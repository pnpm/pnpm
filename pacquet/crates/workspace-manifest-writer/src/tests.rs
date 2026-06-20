//! Ports of pnpm's `addCatalogs` / `updateWorkspaceManifest` catalog tests
//! ([source](https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/test/)).
//!
//! Structural cases assert the parsed shape; the format-sensitive cases
//! assert byte-for-byte, matching pnpm's own expectations.

use std::fs;

use pacquet_catalogs_types::Catalogs;
use tempfile::TempDir;

use crate::{WORKSPACE_MANIFEST_FILENAME, update_workspace_manifest};

fn catalogs(entries: &[(&str, &[(&str, &str)])]) -> Catalogs {
    entries
        .iter()
        .map(|(name, deps)| {
            let map = deps.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            (name.to_string(), map)
        })
        .collect()
}

/// Run `update_workspace_manifest` against `original` (when `Some`) and return
/// the resulting file contents, or `None` when no file exists afterward.
fn run(original: Option<&str>, updated: &Catalogs) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    update_workspace_manifest(dir.path(), updated).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn empty_catalogs_does_not_create_a_file() {
    assert_eq!(run(None, &catalogs(&[])), None);
    assert_eq!(run(None, &catalogs(&[("default", &[])])), None);
    assert_eq!(run(None, &catalogs(&[("foo", &[]), ("bar", &[])])), None);
}

#[test]
fn default_catalog_goes_to_the_catalog_shorthand() {
    let out = run(None, &catalogs(&[("default", &[("foo", "^0.1.2")])])).expect("file written");
    assert_eq!(out, "catalog:\n  foo: ^0.1.2\n");
}

#[test]
fn default_merges_into_existing_catalog_shorthand() {
    let original = "catalog:\n  bar: 3.2.1\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("foo", "^0.1.2")])])).expect("written");
    assert_eq!(out, "catalog:\n  bar: 3.2.1\n  foo: ^0.1.2\n");
}

#[test]
fn default_merges_into_existing_catalogs_default() {
    let original = "catalogs:\n  default:\n    bar: 3.2.1\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("foo", "^0.1.2")])])).expect("written");
    assert_eq!(out, "catalogs:\n  default:\n    bar: 3.2.1\n    foo: ^0.1.2\n");
}

#[test]
fn named_catalogs_create_a_catalogs_block() {
    let out = run(None, &catalogs(&[("bar", &[("def", "3.2.1")]), ("foo", &[("abc", "0.1.2")])]))
        .expect("written");
    assert_eq!(out, "catalogs:\n  bar:\n    def: 3.2.1\n  foo:\n    abc: 0.1.2\n");
}

#[test]
fn named_catalog_added_to_existing_catalogs() {
    let original = "catalogs:\n  foo:\n    ghi: 7.8.9\n";
    let out = run(
        Some(original),
        &catalogs(&[("bar", &[("def", "3.2.1")]), ("foo", &[("abc", "0.1.2")])]),
    )
    .expect("written");
    assert_eq!(out, "catalogs:\n  bar:\n    def: 3.2.1\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n");
}

#[test]
fn adds_a_new_catalog_after_packages() {
    let original = "packages:\n  - '*'\n";
    let out = run(Some(original), &catalogs(&[("default", &[("foo", "1.0.0")])])).expect("written");
    assert_eq!(out, "packages:\n  - '*'\ncatalog:\n  foo: 1.0.0\n");
}

#[test]
fn preserves_quotes_and_appends_new_entry() {
    let original = "catalog:\n  \"bar\": \"2.0.0\"\n  'foo': '1.0.0'\n  qar: 3.0.0\n";
    let out = run(
        Some(original),
        &catalogs(&[(
            "default",
            &[("foo", "1.0.0"), ("bar", "2.0.0"), ("qar", "3.0.0"), ("zoo", "4.0.0")],
        )]),
    )
    .expect("written");
    assert_eq!(
        out,
        "catalog:\n  \"bar\": \"2.0.0\"\n  'foo': '1.0.0'\n  qar: 3.0.0\n  zoo: 4.0.0\n",
    );
}

#[test]
fn preserves_blank_lines_when_inserting_a_catalog_between_fields() {
    let original =
        "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\noverrides:\n  foo: '1.0.0'\n";
    let out = run(Some(original), &catalogs(&[("default", &[("bar", "2.0.0")])])).expect("written");
    assert_eq!(
        out,
        "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\ncatalog:\n  bar: 2.0.0\n\noverrides:\n  foo: '1.0.0'\n",
    );
}

#[test]
fn no_blank_lines_when_original_has_none() {
    let original = "packages:\n  - '*'\nallowBuilds:\n  foo: true\n";
    let out = run(Some(original), &catalogs(&[("default", &[("bar", "2.0.0")])])).expect("written");
    assert_eq!(out, "packages:\n  - '*'\nallowBuilds:\n  foo: true\ncatalog:\n  bar: 2.0.0\n");
}

#[test]
fn catalog_sorts_to_front_with_blank_line_style() {
    let original = "overrides:\n  foo: '2.0.0'\n\npackages:\n  - '*'\n";
    let out = run(Some(original), &catalogs(&[("default", &[("bar", "1.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  bar: 1.0.0\n\noverrides:\n  foo: '2.0.0'\n\npackages:\n  - '*'\n");
}

#[test]
fn inserts_entry_in_sorted_position() {
    let original = "catalog:\n  apple: '1.0.0'\n  mango: '2.0.0'\n  zebra: '3.0.0'\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("banana", "4.0.0")])])).expect("written");
    assert_eq!(
        out,
        "catalog:\n  apple: '1.0.0'\n  banana: 4.0.0\n  mango: '2.0.0'\n  zebra: '3.0.0'\n",
    );
}

#[test]
fn appends_entry_when_block_is_unordered() {
    let original = "catalog:\n  zebra: '1.0.0'\n  apple: '2.0.0'\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("mango", "3.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  zebra: '1.0.0'\n  apple: '2.0.0'\n  mango: 3.0.0\n");
}

#[test]
fn no_op_when_entry_already_present_with_same_specifier() {
    let original = "catalog:\n  # keep this comment\n  foo: ^1.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("foo", "^1.0.0")])])).expect("written");
    assert_eq!(out, original);
}

#[test]
fn updates_named_catalog_value_preserving_comment() {
    let original = "catalogs:\n  react:\n    # pinned by the platform team\n    react: 18.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("react", &[("react", "18.2.0")])])).expect("written");
    assert_eq!(out, "catalogs:\n  react:\n    # pinned by the platform team\n    react: 18.2.0\n");
}

#[test]
fn inserts_entry_into_a_four_space_indented_block() {
    let original = "catalogs:\n    react:\n        react: 18.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("react", &[("react-dom", "18.0.0")])])).expect("written");
    assert_eq!(out, "catalogs:\n    react:\n        react: 18.0.0\n        react-dom: 18.0.0\n");
}

#[test]
fn quotes_scoped_package_keys() {
    // A key starting with `@` cannot be a YAML plain scalar, so it must be
    // quoted — both when creating the block and when adding an entry.
    let out = run(None, &catalogs(&[("default", &[("@pnpm.e2e/foo", "1.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  '@pnpm.e2e/foo': 1.0.0\n");

    let out =
        run(Some(&out), &catalogs(&[("default", &[("@pnpm.e2e/bar", "2.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  '@pnpm.e2e/bar': 2.0.0\n  '@pnpm.e2e/foo': 1.0.0\n");
}

#[test]
fn preserves_comment_when_inserting_before_commented_entry() {
    let original = "catalog:\n  apple: 1.0.0\n  # note about zebra\n  zebra: 3.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("mango", "2.0.0")])])).expect("written");
    assert_eq!(
        out,
        "catalog:\n  apple: 1.0.0\n  mango: 2.0.0\n  # note about zebra\n  zebra: 3.0.0\n",
    );
}

/// Run `set_config_dependency` against `original` (when `Some`) and return
/// the resulting file contents.
fn run_config_dep(original: Option<&str>, name: &str, specifier: &str) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_config_dependency(dir.path(), name, specifier).expect("update succeeds");
    fs::read_to_string(&path).expect("file written")
}

#[test]
fn config_dependency_creates_block_when_absent() {
    let out = run_config_dep(None, "@pnpm.e2e/foo", "1.0.0");
    assert_eq!(out, "configDependencies:\n  '@pnpm.e2e/foo': 1.0.0\n");
}

#[test]
fn config_dependency_added_to_existing_block() {
    let original = "configDependencies:\n  '@pnpm.e2e/bar': 2.0.0\n";
    let out = run_config_dep(Some(original), "@pnpm.e2e/foo", "1.0.0");
    assert_eq!(out, "configDependencies:\n  '@pnpm.e2e/bar': 2.0.0\n  '@pnpm.e2e/foo': 1.0.0\n");
}

#[test]
fn config_dependency_upserts_existing_entry() {
    let original = "configDependencies:\n  '@pnpm.e2e/foo': 1.0.0\n";
    let out = run_config_dep(Some(original), "@pnpm.e2e/foo", "2.0.0");
    assert_eq!(out, "configDependencies:\n  '@pnpm.e2e/foo': 2.0.0\n");
}

#[test]
fn config_dependency_preserves_other_keys_and_comments() {
    let original = "# top comment\nstoreDir: ../store\n";
    let out = run_config_dep(Some(original), "pnpm-plugin-x", "1.2.3");
    assert!(out.contains("# top comment"), "comment preserved");
    assert!(out.contains("storeDir: ../store"), "existing key preserved");
    assert!(out.contains("configDependencies:\n  pnpm-plugin-x: 1.2.3"), "block appended");
}

#[test]
fn config_dependency_noop_when_unchanged_returns_false() {
    use crate::{edit, model::Manifest};

    let original = "configDependencies:\n  '@pnpm.e2e/foo': 1.0.0\n";

    let mut manifest = Manifest::parse(Some(original)).unwrap();
    assert!(
        !edit::add_config_dependency(&mut manifest, "@pnpm.e2e/foo", "1.0.0").unwrap(),
        "re-adding the same specifier should report no change",
    );

    let mut manifest = Manifest::parse(Some(original)).unwrap();
    assert!(
        edit::add_config_dependency(&mut manifest, "@pnpm.e2e/foo", "2.0.0").unwrap(),
        "changing the specifier should report a change",
    );
}
