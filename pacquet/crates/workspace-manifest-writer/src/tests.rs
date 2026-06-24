//! Ports of pnpm's `addCatalogs` / `updateWorkspaceManifest` catalog tests
//! ([source](https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/test/)).
//!
//! Structural cases assert the parsed shape; the format-sensitive cases
//! assert byte-for-byte, matching pnpm's own expectations.

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
