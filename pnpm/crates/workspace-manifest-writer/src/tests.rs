//! Tests for the workspace-manifest catalog writer.
//!
//! Structural cases assert the parsed shape; the format-sensitive cases
//! assert byte-for-byte.

use std::{fs, path::PathBuf};

use indexmap::IndexMap;
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
fn config_dependencies_batch_updates_all_entries_in_one_manifest() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "# preserved comment\nconfigDependencies:\n  existing-package: 0.1.0\n")
        .expect("seed manifest");

    crate::set_config_dependencies(
        dir.path(),
        [("@pnpm.e2e/foo", "1.0.0"), ("@pnpm.e2e/bar", "2.0.0")],
    )
    .expect("batch update succeeds");

    let out = fs::read_to_string(path).expect("read updated manifest");
    for expected in [
        "# preserved comment",
        "existing-package: 0.1.0",
        "'@pnpm.e2e/foo': 1.0.0",
        "'@pnpm.e2e/bar': 2.0.0",
    ] {
        assert!(out.contains(expected), "manifest contains {expected:?}");
    }
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

/// Run `set_allow_builds` against `original` (when `Some`) and return the
/// resulting file contents (or `None` when no file exists afterward).
fn run_allow_builds(original: Option<&str>, entries: &[(&str, bool)]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_allow_builds(dir.path(), entries.iter().copied()).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn allow_builds_creates_block_when_absent() {
    let out = run_allow_builds(None, &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true\n"));
}

#[test]
fn allow_builds_writes_boolean_values_unquoted() {
    let out = run_allow_builds(None, &[("esbuild", true), ("@scope/pkg", false)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  '@scope/pkg': false\n  esbuild: true\n"));
}

#[test]
fn allow_builds_upserts_existing_entry() {
    let original = "allowBuilds:\n  esbuild: false\n";
    let out = run_allow_builds(Some(original), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true\n"));
}

#[test]
fn allow_builds_no_op_when_unchanged_keeps_file() {
    let original = "allowBuilds:\n  esbuild: true\n";
    let out = run_allow_builds(Some(original), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some(original));
}

#[test]
fn allow_builds_preserves_other_keys_and_comments() {
    let original = "# top comment\nstoreDir: ../store\n";
    let out = run_allow_builds(Some(original), &[("esbuild", true)]).expect("file written");
    assert!(out.contains("# top comment"), "comment preserved");
    assert!(out.contains("storeDir: ../store"), "existing key preserved");
    assert!(out.contains("allowBuilds:\n  esbuild: true"), "block appended");
}

#[cfg(unix)]
#[test]
fn set_allow_builds_replaces_a_symlinked_manifest_without_following_it() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("temp dir");
    // A file outside the manifest that a malicious symlink would target.
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, "").expect("seed outside file");
    let manifest = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    symlink(&outside, &manifest).expect("symlink the manifest to the outside file");

    crate::set_allow_builds(dir.path(), [("esbuild", true)]).expect("update succeeds");

    // The atomic rename replaces the symlink's directory entry, so the
    // outside target is untouched and the manifest is now a regular file.
    assert_eq!(fs::read_to_string(&outside).expect("read outside"), "");
    assert!(
        !fs::symlink_metadata(&manifest).expect("stat manifest").file_type().is_symlink(),
        "the manifest should no longer be a symlink",
    );
    assert_eq!(
        fs::read_to_string(&manifest).expect("read manifest"),
        "allowBuilds:\n  esbuild: true\n",
    );
}

#[test]
fn allow_builds_upserts_a_key_containing_a_colon() {
    // Artifact allow-build keys keep the full pkgId, which contains `:`
    // (e.g. a tarball/git URL). The upsert must find and toggle the
    // existing entry instead of appending a duplicate — which a
    // first-colon line scan would do by truncating the key.
    let key = "foo@https://example.com/foo.tgz";
    let original = format!("allowBuilds:\n  '{key}': false\n");
    let out = run_allow_builds(Some(&original), &[(key, true)]).expect("file written");
    assert_eq!(out, format!("allowBuilds:\n  '{key}': true\n"));
    assert_eq!(out.matches(key).count(), 1, "exactly one entry, no duplicate: {out}");
}

#[test]
fn allow_builds_creates_and_round_trips_a_colon_key() {
    let key = "foo@https://example.com/foo.tgz";
    let created = run_allow_builds(None, &[(key, true)]).expect("file written");
    assert!(created.contains(key), "key written verbatim: {created}");
    // Re-upserting the same value is a no-op (the entry is found, not duplicated).
    let same = run_allow_builds(Some(&created), &[(key, true)]);
    assert_eq!(same.as_deref(), Some(created.as_str()), "idempotent: {created}");
    // Toggling flips the existing entry rather than appending a duplicate.
    let toggled = run_allow_builds(Some(&created), &[(key, false)]).expect("written");
    assert_eq!(toggled.matches(key).count(), 1, "no duplicate after toggle: {toggled}");
}

#[test]
fn allow_builds_rejects_a_manifest_with_duplicate_keys() {
    // A repo-controlled manifest with duplicate `allowBuilds` keys is
    // rejected at parse time (`DuplicateMappingKey`), so `set_allow_builds`
    // errors and writes nothing rather than rewriting only the first
    // occurrence and leaving the effective (last) value untouched. The
    // policy change fails loudly instead of being silently bypassed.
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "allowBuilds:\n  esbuild: false\n  esbuild: true\n";
    fs::write(&path, original).expect("seed manifest");

    let result = crate::set_allow_builds(dir.path(), [("esbuild", false)]);
    assert!(
        matches!(result, Err(crate::UpdateWorkspaceManifestError::Parse { .. })),
        "duplicate keys must be rejected, got {result:?}",
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read manifest"),
        original,
        "the manifest is left unchanged when the update fails",
    );
}

fn patched_deps(entries: &[(&str, &str)]) -> IndexMap<String, String> {
    entries.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

/// Run `set_patched_dependencies` against `original` (when `Some`) and return
/// the resulting file contents.
fn run_patched_deps(original: Option<&str>, entries: &[(&str, &str)]) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_patched_dependencies(dir.path(), &patched_deps(entries)).expect("update succeeds");
    fs::read_to_string(&path).expect("file written")
}

fn run_patched_deps_path(original: Option<&str>, entries: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_patched_dependencies(dir.path(), &patched_deps(entries)).expect("update succeeds");
    (dir, path)
}

#[test]
fn patched_dependency_creates_block_when_absent() {
    let out = run_patched_deps(None, &[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")]);
    assert_eq!(out, "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n");
}

#[test]
fn patched_dependency_quotes_scoped_keys_and_slash_paths() {
    let out = run_patched_deps(
        None,
        &[("@pnpm.e2e/console-log", "patches/@pnpm.e2e__console-log.patch")],
    );
    assert_eq!(
        out,
        "patchedDependencies:\n  '@pnpm.e2e/console-log': patches/@pnpm.e2e__console-log.patch\n",
    );
}

#[test]
fn patched_dependency_preserves_existing_manifest_content() {
    let original = "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(
        Some(original),
        &[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")],
    );
    assert_eq!(
        out,
        "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\ncatalog:\n  react: 18.2.0\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
    );
}

#[test]
fn patched_dependency_noops_when_unchanged() {
    use crate::{edit, model::Manifest};

    let original = "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n";
    let deps = patched_deps(&[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")]);
    let mut manifest = Manifest::parse(Some(original)).unwrap();
    assert!(
        !edit::add_patched_dependencies(&mut manifest, &deps).unwrap(),
        "re-adding the same patch entry should report no change",
    );
    assert_eq!(manifest.into_text(), original);
}

#[test]
fn patched_dependency_removes_omitted_entries() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-negative@1.0.0: patches/is-negative@1.0.0.patch\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(
        Some(original),
        &[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")],
    );

    assert_eq!(
        out,
        "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n\ncatalog:\n  react: 18.2.0\n",
    );
}

#[test]
fn patched_dependency_removes_empty_block() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, "packages:\n  - '*'\n\ncatalog:\n  react: 18.2.0\n");
}

#[test]
fn patched_dependency_removes_empty_or_null_block() {
    let empty = "packages:\n  - '*'\n\npatchedDependencies:\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(empty), &[]);
    assert_eq!(out, "packages:\n  - '*'\n\ncatalog:\n  react: 18.2.0\n");

    let null = "packages:\n  - '*'\n\npatchedDependencies: null\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(null), &[]);
    assert_eq!(out, "packages:\n  - '*'\n\ncatalog:\n  react: 18.2.0\n");
}

#[test]
fn patched_dependency_remove_preserves_successor_comments() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive: patches/is-positive.patch\n\n# catalog pins\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, "packages:\n  - '*'\n\n# catalog pins\ncatalog:\n  react: 18.2.0\n");
}

#[test]
fn patched_dependency_removes_empty_last_block() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, "packages:\n  - '*'\n\n");
}

#[test]
fn patched_dependency_removes_manifest_when_last_setting_is_removed() {
    let original = "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n";
    let (_dir, path) = run_patched_deps_path(Some(original), &[]);

    assert!(!path.exists(), "empty pnpm-workspace.yaml should be removed");
}

#[test]
fn patched_dependency_empty_map_does_not_create_manifest() {
    let (_dir, path) = run_patched_deps_path(None, &[]);

    assert!(!path.exists(), "empty patchedDependencies should not create pnpm-workspace.yaml");
}

#[test]
fn patched_dependency_empty_map_preserves_manifest_without_patch_block() {
    let original = "packages:\n  - '*'\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, original);
}

#[test]
fn patched_dependency_missing_decoded_block_returns_original_text_when_removing_block() {
    use crate::{edit, model::Manifest};

    let original = "packages:\n  - '*'\n";
    let mut manifest = Manifest::parse(Some(original)).unwrap();
    manifest.patched_dependencies = Some(IndexMap::from([(
        "is-positive".to_string(),
        "patches/is-positive.patch".to_string(),
    )]));

    assert!(edit::add_patched_dependencies(&mut manifest, &IndexMap::new()).unwrap());
    assert_eq!(manifest.into_text(), original);
}

#[test]
fn patched_dependency_missing_decoded_mapping_keeps_text_before_inserting_new_block() {
    use crate::{edit, model::Manifest};

    let original = "packages:\n  - '*'\n";
    let mut manifest = Manifest::parse(Some(original)).unwrap();
    manifest.patched_dependencies = Some(IndexMap::from([(
        "is-negative".to_string(),
        "patches/is-negative.patch".to_string(),
    )]));
    let deps = patched_deps(&[("is-positive", "patches/is-positive.patch")]);

    assert!(edit::add_patched_dependencies(&mut manifest, &deps).unwrap());

    let text = manifest.into_text();
    assert!(text.contains("packages:\n  - '*'\n"), "text: {text}");
    assert!(text.contains("is-positive: patches/is-positive.patch"), "text: {text}");
    assert!(!text.contains("is-negative"), "text: {text}");
}

#[test]
fn write_or_remove_manifest_ignores_missing_empty_manifest() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let manifest = crate::model::Manifest::parse(Some("")).expect("empty manifest");

    crate::write_or_remove_manifest(&path, manifest).expect("remove missing empty manifest");

    assert!(!path.exists());
}

#[test]
fn set_patched_dependencies_reports_read_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::create_dir(&path).expect("create manifest dir");

    let err = crate::set_patched_dependencies(
        dir.path(),
        &patched_deps(&[("is-positive", "patches/is-positive.patch")]),
    )
    .expect_err("manifest directory should fail to read");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Read { .. }));
}

#[test]
fn write_or_remove_manifest_reports_remove_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::create_dir(&path).expect("create manifest dir");
    let manifest = crate::model::Manifest::parse(Some("")).expect("empty manifest");

    let err =
        crate::write_or_remove_manifest(&path, manifest).expect_err("directory remove should fail");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Remove { .. }));
}

#[test]
fn write_or_remove_manifest_reports_write_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("missing").join(WORKSPACE_MANIFEST_FILENAME);
    let manifest = crate::model::Manifest::parse(Some("packages:\n  - '*'\n")).expect("manifest");

    let err =
        crate::write_or_remove_manifest(&path, manifest).expect_err("missing parent should fail");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Write { .. }));
}

fn overrides(entries: &[(&str, &str)]) -> IndexMap<String, String> {
    entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Run `set_overrides` against `original` (when `Some`) and return the
/// resulting file contents, or `None` when no file exists afterward.
fn run_overrides(original: Option<&str>, entries: &IndexMap<String, String>) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_overrides(
        dir.path(),
        entries.iter().map(|(key, value)| (key.as_str(), value.as_str())),
    )
    .expect("set_overrides succeeds");
    fs::read_to_string(&path).ok()
}

/// Run `set_audit_ignore_ghsas` against `original` (when `Some`) and return
/// the resulting file contents, or `None` when no file exists afterward.
fn run_ignore_ghsas(original: Option<&str>, ghsas: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    let owned: Vec<String> = ghsas.iter().map(ToString::to_string).collect();
    crate::set_audit_ignore_ghsas(dir.path(), &owned).expect("set_audit_ignore_ghsas succeeds");
    fs::read_to_string(&path).ok()
}

/// Run `remove_overrides` against `original` and return the resulting file
/// contents, or `None` when no file exists afterward.
fn run_remove_overrides(original: Option<&str>, selectors: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    let selectors: Vec<String> = selectors.iter().copied().map(ToString::to_string).collect();
    crate::remove_overrides(dir.path(), &selectors).expect("remove succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn overrides_block_is_created() {
    let out = run_overrides(None, &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, "overrides:\n  foo@<1.0.1: ^1.0.1\n");
}

#[test]
fn overrides_quote_keys_and_values_that_need_it() {
    let out =
        run_overrides(None, &overrides(&[("@scope/foo@>=1.0.0", ">=1.0.1")])).expect("written");
    assert_eq!(out, "overrides:\n  '@scope/foo@>=1.0.0': '>=1.0.1'\n");
}

#[test]
fn overrides_merge_into_an_existing_block() {
    let original = "overrides:\n  bar@1: 2\n";
    let out =
        run_overrides(Some(original), &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, "overrides:\n  bar@1: 2\n  foo@<1.0.1: ^1.0.1\n");
}

#[test]
fn overrides_are_added_after_packages() {
    let original = "packages:\n  - '*'\n";
    let out =
        run_overrides(Some(original), &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, "packages:\n  - '*'\noverrides:\n  foo@<1.0.1: ^1.0.1\n");
}

#[test]
fn overrides_noop_when_already_present() {
    let original = "overrides:\n  foo@<1.0.1: ^1.0.1\n";
    let out =
        run_overrides(Some(original), &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, original);
}

#[test]
fn audit_config_block_is_created() {
    let out = run_ignore_ghsas(None, &["GHSA-aaaa-bbbb-cccc"]).expect("written");
    assert_eq!(out, "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n");
}

#[test]
fn audit_config_block_with_multiple_ghsas() {
    let out =
        run_ignore_ghsas(None, &["GHSA-aaaa-bbbb-cccc", "GHSA-dddd-eeee-ffff"]).expect("written");
    assert_eq!(
        out,
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n    - GHSA-dddd-eeee-ffff\n",
    );
}

#[test]
fn ignore_ghsas_replaces_an_existing_list() {
    let original = "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-aaaa-bbbb-cccc", "GHSA-dddd-eeee-ffff"])
        .expect("written");
    assert_eq!(
        out,
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n    - GHSA-dddd-eeee-ffff\n",
    );
}

#[test]
fn ignore_ghsas_adds_key_to_existing_audit_config() {
    let original = "auditConfig:\n  other: keep\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-aaaa-bbbb-cccc"]).expect("written");
    assert_eq!(out, "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n  other: keep\n");
}

#[test]
fn ignore_ghsas_noop_when_already_present() {
    let original = "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-aaaa-bbbb-cccc"]).expect("written");
    assert_eq!(out, original);
}

#[test]
fn ignore_ghsas_empty_removes_the_block() {
    let original = "packages:\n  - '*'\nauditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn ignore_ghsas_empty_preserves_sibling_audit_config_keys() {
    let original = "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n  other: keep\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "auditConfig:\n  other: keep\n");
}

#[test]
fn ignore_ghsas_empty_with_sibling_only_is_a_noop() {
    let original = "auditConfig:\n  other: keep\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, original);
}

#[test]
fn ignore_ghsas_refuses_inline_flow_audit_config() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // A hand-written flow-style auditConfig: the block-splice writer can't
    // safely edit it, so it must refuse instead of corrupting the file.
    fs::write(&path, "auditConfig: { ignoreGhsas: [GHSA-aaaa-bbbb-cccc] }\n").expect("seed");

    let err = crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-dddd-eeee-ffff".to_string()])
        .expect_err("must refuse an inline auditConfig");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "auditConfig: { ignoreGhsas: [GHSA-aaaa-bbbb-cccc] }\n");
}

/// Run `set_minimum_release_age_excludes` against `original` and return the
/// resulting file contents, or `None` when no file exists afterward.
fn run_age_excludes(original: Option<&str>, excludes: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    let owned: Vec<String> = excludes.iter().map(ToString::to_string).collect();
    crate::set_minimum_release_age_excludes(dir.path(), &owned)
        .expect("set_minimum_release_age_excludes succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn minimum_release_age_exclude_block_is_created() {
    let out = run_age_excludes(None, &["foo@1.0.0", "bar@2.0.0"]).expect("written");
    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - bar@2.0.0\n");
}

#[test]
fn minimum_release_age_exclude_added_after_packages() {
    let original = "packages:\n  - '*'\n";
    let out = run_age_excludes(Some(original), &["foo@1.0.0"]).expect("written");
    assert_eq!(out, "packages:\n  - '*'\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n");
}

#[test]
fn minimum_release_age_exclude_replaces_existing_block() {
    let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
    let out = run_age_excludes(Some(original), &["foo@1.0.0", "bar@2.0.0"]).expect("written");
    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - bar@2.0.0\n");
}

#[test]
fn minimum_release_age_exclude_noop_when_unchanged() {
    let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
    let out = run_age_excludes(Some(original), &["foo@1.0.0"]).expect("written");
    assert_eq!(out, original);
}

#[test]
fn minimum_release_age_exclude_empty_removes_the_block() {
    let original = "packages:\n  - '*'\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n";
    let out = run_age_excludes(Some(original), &[]).expect("written");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn set_overrides_refuses_to_clobber_a_non_scalar_value() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // A hand-written parent-scoped (object) override at the same selector key.
    fs::write(&path, "overrides:\n  foo@<2.0.0:\n    bar: 1.0.0\n").expect("seed manifest");

    let err = crate::set_overrides(dir.path(), [("foo@<2.0.0", "^2.0.0")])
        .expect_err("must refuse to overwrite a non-scalar override");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::OverrideConflict { .. }));
    // The original object value is left untouched.
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides:\n  foo@<2.0.0:\n    bar: 1.0.0\n");
}

#[test]
fn set_overrides_refuses_inline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // A hand-written flow-style overrides block can't be block-spliced safely.
    fs::write(&path, "overrides: { foo: 1.0.0 }\n").expect("seed manifest");

    let err = crate::set_overrides(dir.path(), [("bar@<2.0.0", "^2.0.0")])
        .expect_err("must refuse an inline overrides block");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides: { foo: 1.0.0 }\n");
}

#[test]
fn ignore_ghsas_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);

    // A newline in the value would splice into a multi-line scalar.
    let err = crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-aaaa\nbreak".to_string()])
        .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
    assert!(!path.exists(), "nothing should be written");
}

#[test]
fn minimum_release_age_excludes_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");

    let err =
        crate::set_minimum_release_age_excludes(dir.path(), &["foo\r\nbar@1.0.0".to_string()])
            .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
}

#[test]
fn set_overrides_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");

    let err = crate::set_overrides(dir.path(), [("foo@<2.0.0\nx", "^2.0.0")])
        .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
}

#[test]
fn remove_overrides_drops_only_the_named_entry() {
    let original = "overrides:\n  foo: link:../foo\n  bar: link:../bar\n  baz: 1.0.0\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides:\n  bar: link:../bar\n  baz: 1.0.0\n");
}

#[test]
fn remove_overrides_drops_the_block_when_emptied_but_keeps_siblings() {
    let original = "packages:\n  - '*'\noverrides:\n  foo: link:../foo\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn remove_overrides_deletes_the_file_when_nothing_remains() {
    let original = "overrides:\n  foo: link:../foo\n  bar: link:../bar\n";
    assert_eq!(run_remove_overrides(Some(original), &["foo", "bar"]), None);
}

#[test]
fn remove_overrides_is_a_noop_for_absent_selectors() {
    let original = "overrides:\n  foo: link:../foo\n";
    let out = run_remove_overrides(Some(original), &["missing"]).expect("file kept");
    assert_eq!(out, original);
}

#[test]
fn remove_overrides_is_a_noop_when_the_manifest_is_missing() {
    assert_eq!(run_remove_overrides(None, &["foo"]), None);
}

#[test]
fn remove_overrides_handles_flow_style_mappings() {
    let original = "overrides: { foo: link:../foo, bar: 1.0.0 }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides:\n  bar: 1.0.0\n");
}

#[test]
fn remove_overrides_drops_a_flow_style_block_when_emptied() {
    let original = "packages:\n  - '*'\noverrides: { foo: link:../foo }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn remove_overrides_preserves_non_string_entries_in_block_style() {
    let original = "overrides:\n  foo: link:../foo\n  bar:\n    nested: value\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides:\n  bar:\n    nested: value\n");
}

#[test]
fn remove_overrides_keeps_block_when_only_non_string_entry_remains() {
    // Removing the last string entry must not delete the block while a
    // non-string entry (which the decoded map drops) is still present.
    let original = "overrides:\n  foo: link:../foo\n  bar:\n    nested: value\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert!(out.contains("bar:"), "non-string override must survive: {out}");
}

#[test]
fn remove_overrides_leaves_flow_style_block_with_non_string_entries_untouched() {
    // A flow-style block can only be rewritten wholesale, and the decoded map
    // cannot reserialize the non-string `bar`, so the file is left as-is rather
    // than dropping data.
    let original = "overrides: { foo: link:../foo, bar: { nested: value } }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, original);
}

// --- generic top-level field set/delete (pnpm config set / delete) ---

fn run_update_field(
    original: Option<&str>,
    key: &str,
    value: &serde_json::Value,
) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::update_manifest_field(&path, key, value).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn set_scalar_field_into_existing_file() {
    let out =
        run_update_field(Some("storeDir: ~/store\n"), "fetchTimeout", &serde_json::json!(1000))
            .expect("file written");
    let parsed: indexmap::IndexMap<String, serde_json::Value> =
        serde_saphyr::from_str(&out).expect("parse");
    assert_eq!(parsed["storeDir"], serde_json::json!("~/store"));
    assert_eq!(parsed["fetchTimeout"], serde_json::json!(1000));
}

#[test]
fn set_object_field_with_json() {
    let value = serde_json::json!({
        "@babel/parser": { "peerDependencies": { "@babel/types": "*" } },
        "jest-circus": { "dependencies": { "slash": "3" } },
    });
    let out = run_update_field(None, "packageExtensions", &value).expect("file written");
    let parsed: serde_json::Value = serde_saphyr::from_str(&out).expect("parse");
    assert_eq!(parsed["packageExtensions"], value);
}

#[test]
fn delete_last_field_removes_file() {
    let out = run_update_field(
        Some("virtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    );
    assert_eq!(out, None);
}

#[test]
fn delete_unset_field_is_noop() {
    let out = run_update_field(Some("cacheDir: ~/cache\n"), "storeDir", &serde_json::Value::Null)
        .expect("file kept");
    let parsed: indexmap::IndexMap<String, serde_json::Value> =
        serde_saphyr::from_str(&out).expect("parse");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed["cacheDir"], serde_json::json!("~/cache"));
}
