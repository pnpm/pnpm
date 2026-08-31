//! Tests for the workspace-manifest catalog writer.
//!
//! Structural cases assert the parsed shape; the format-sensitive cases
//! assert byte-for-byte.

use std::{fs, path::PathBuf};

use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::PackageManifest;
use tempfile::TempDir;

use crate::{
    UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, update_workspace_manifest,
};

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
    run_with(
        original,
        &UpdateWorkspaceManifestOptions { updated_catalogs: Some(updated), ..Default::default() },
    )
}

fn run_with(original: Option<&str>, opts: &UpdateWorkspaceManifestOptions<'_>) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    update_workspace_manifest(dir.path(), opts).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

/// [`run_with`] for the cleanup cases: merge `updated` (when `Some`), then
/// run the `catalogPrune` pass over `projects`.
fn run_cleanup(
    original: Option<&str>,
    updated: Option<&Catalogs>,
    projects: &[&PackageManifest],
) -> Option<String> {
    run_with(
        original,
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: updated,
            catalog_prune: true,
            all_projects: projects,
            ..Default::default()
        },
    )
}

fn project(manifest: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("project/package.json"), manifest)
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

/// Run `set_config_dependencies` with a single entry against `original`
/// (when `Some`) and return the resulting file contents.
fn run_config_dep(original: Option<&str>, name: &str, specifier: &str) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_config_dependencies(dir.path(), [(name, specifier)]).expect("update succeeds");
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
    assert_eq!(
        out,
        "# preserved comment\nconfigDependencies:\n  '@pnpm.e2e/bar': 2.0.0\n  '@pnpm.e2e/foo': 1.0.0\n  existing-package: 0.1.0\n",
    );
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

/// Run `scaffold_allow_builds` against `original` (when `Some`) and
/// return the resulting file contents (or `None` when no file exists
/// afterward).
fn run_scaffold_allow_builds(original: Option<&str>, names: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::scaffold_allow_builds(dir.path(), names.iter().copied()).expect("update succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn scaffold_allow_builds_creates_block_when_absent() {
    let out = run_scaffold_allow_builds(Some("packages: []\n"), &["es5-ext"]);
    assert_eq!(
        out.as_deref(),
        Some("packages: []\nallowBuilds:\n  es5-ext: set this to true or false\n"),
    );
}

#[test]
fn scaffold_allow_builds_leaves_a_decided_entry_alone() {
    let original = "allowBuilds:\n  esbuild: false\n";
    let out = run_scaffold_allow_builds(Some(original), &["esbuild", "es5-ext"]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds:\n  es5-ext: set this to true or false\n  esbuild: false\n"),
    );
}

/// Every install that keeps ignoring the same build re-runs the scaffold;
/// the second one must not rewrite the file (and bump its mtime).
#[test]
fn scaffold_allow_builds_no_op_when_already_scaffolded_keeps_file() {
    let original = "allowBuilds:\n  es5-ext: set this to true or false\n";
    let out = run_scaffold_allow_builds(Some(original), &["es5-ext"]);
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

    assert_eq!(out, "packages:\n  - '*'\n");
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

fn run_allow_builds_clearing_legacy(
    original: Option<&str>,
    entries: &[(&str, bool)],
) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    if let Some(text) = original {
        fs::write(&path, text).expect("seed manifest");
    }
    crate::set_allow_builds_clearing_legacy(dir.path(), entries.iter().copied())
        .expect("update succeeds");
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
fn ignore_ghsas_targets_the_canonical_audit_ignore_list() {
    let original = "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-dddd-eeee-ffff"]).expect("written");
    assert_eq!(out, "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-dddd-eeee-ffff\n");
}

#[test]
fn ignore_ghsas_removes_the_shadowed_deprecated_list_when_both_are_present() {
    let original = "audit:\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\nauditConfig:\n  ignoreGhsas:\n    - GHSA-1111-2222-3333\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-dddd-eeee-ffff"]).expect("written");
    assert_eq!(out, "audit:\n  ignore:\n    - GHSA-dddd-eeee-ffff\n");
}

#[test]
fn ignore_ghsas_empty_removes_audit_ignore_and_keeps_siblings() {
    let original = "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "audit:\n  ignorePrune: true\n");
}

#[test]
fn ignore_ghsas_empty_removes_the_audit_block_when_ignore_is_its_only_key() {
    let original = "packages:\n  - '.'\naudit:\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "packages:\n  - '.'\n");
}

#[test]
fn ignore_ghsas_edits_an_inline_flow_audit_config() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "auditConfig: { other: keep, ignoreGhsas: [GHSA-aaaa-bbbb-cccc] }\n")
        .expect("seed");

    crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-dddd-eeee-ffff".to_string()])
        .expect("set_audit_ignore_ghsas succeeds");

    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "auditConfig: { other: keep, ignoreGhsas: [ GHSA-dddd-eeee-ffff ] }\n");
}

#[test]
fn ignore_ghsas_refuses_a_multiline_flow_audit_config() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // Rebuilding a multi-line flow mapping onto one line would drop the
    // comments between its entries, so the write is refused instead.
    let original = "auditConfig: {\n  ignoreGhsas: [GHSA-aaaa-bbbb-cccc], # pinned\n}\n";
    fs::write(&path, original).expect("seed");

    let err = crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-dddd-eeee-ffff".to_string()])
        .expect_err("must refuse a multi-line inline auditConfig");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, original);
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
fn minimum_release_age_excludes_are_added_to_the_local_manifest_values() {
    let added = ["local@2.0.0".to_string()];
    let out = run_with(
        Some("minimumReleaseAgeExclude:\n  - local@1.0.0\n"),
        &UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: &added,
            ..Default::default()
        },
    )
    .expect("written");

    assert_eq!(out, "minimumReleaseAgeExclude:\n  - local@1.0.0 || 2.0.0\n");
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
fn set_overrides_edits_an_inline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "overrides: { foo: 1.0.0 } # pinned\n").expect("seed manifest");

    crate::set_overrides(dir.path(), [("bar", "^2.0.0")]).expect("set_overrides succeeds");

    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides: { bar: ^2.0.0, foo: 1.0.0 } # pinned\n");
}

#[test]
fn set_overrides_updates_an_entry_of_an_inline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "overrides: { 'foo': \"1.0.0\", bar: 2.0.0 }\n").expect("seed manifest");

    crate::set_overrides(dir.path(), [("foo", "^3.0.0")]).expect("set_overrides succeeds");

    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides: { 'foo': ^3.0.0, bar: 2.0.0 }\n");
}

#[test]
fn set_overrides_refuses_a_multiline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "overrides: {\n  foo: 1.0.0, # pinned\n}\n";
    fs::write(&path, original).expect("seed manifest");

    let err = crate::set_overrides(dir.path(), [("bar", "^2.0.0")])
        .expect_err("must refuse a multi-line inline overrides block");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, original);
}

#[test]
fn set_allow_builds_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);

    // A newline in a package name (e.g. a crafted `--allow-build`) would
    // splice into a multi-line scalar and corrupt the block.
    let err = crate::set_allow_builds(dir.path(), [("esbuild\ninjected: true", true)])
        .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
    assert!(!path.exists(), "nothing should be written");
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

/// `saveCatalogName` is unconstrained — it comes from
/// `pnpm-workspace.yaml`, `PNPM_CONFIG_SAVE_CATALOG_NAME`, or
/// `--save-catalog-name`. A newline in it renders as a YAML block scalar
/// that the splice would write into the middle of the `catalogs:`
/// header; U+2028 / U+2029 are subtler, folding the scalar so the name
/// parses back with the folding indentation embedded in it.
#[test]
fn add_catalogs_rejects_control_characters() {
    let original = "packages:\n  - pkgs/*\n";
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, original).expect("seed manifest");

    for updated in [
        catalogs(&[("shared\n  injected: oops", &[("foo", "^1.0.0")])]),
        catalogs(&[("shared", &[("foo\nbar", "^1.0.0")])]),
        catalogs(&[("shared", &[("foo", "^1.0.0\nbaz: qux")])]),
        catalogs(&[("sha\u{2028}red", &[("foo", "^1.0.0")])]),
        catalogs(&[("sha\u{2029}red", &[("foo", "^1.0.0")])]),
    ] {
        let err = update_workspace_manifest(
            dir.path(),
            &UpdateWorkspaceManifestOptions {
                updated_catalogs: Some(&updated),
                ..Default::default()
            },
        )
        .expect_err("must reject a line-break character");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
        assert_eq!(fs::read_to_string(&path).expect("manifest kept"), original);
    }
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
    assert_eq!(out, "overrides: { bar: 1.0.0 }\n");
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
fn allow_builds_clearing_legacy_drops_every_legacy_key_in_the_same_write() {
    let original = "packages:\n  - '*'\nonlyBuiltDependencies:\n  - esbuild\nonlyBuiltDependenciesFile: allowed.json\nneverBuiltDependencies:\n  - fsevents\nignoredBuiltDependencies:\n  - foo\n";
    let out =
        run_allow_builds_clearing_legacy(Some(original), &[("esbuild", true)]).expect("file kept");
    eprintln!("MANIFEST:\n{out}\n");
    assert_eq!(out, "packages:\n  - '*'\nallowBuilds:\n  esbuild: true\n");
}

#[test]
fn allow_builds_clearing_legacy_deletes_the_file_when_nothing_remains() {
    let original = "onlyBuiltDependencies:\n  - esbuild\n";
    assert_eq!(run_allow_builds_clearing_legacy(Some(original), &[]), None);
}

#[test]
fn allow_builds_clearing_legacy_is_a_noop_when_no_legacy_key_is_present() {
    let original = "packages:\n  - '*'\nallowBuilds:\n  esbuild: true\n";
    let out = run_allow_builds_clearing_legacy(Some(original), &[]).expect("file kept");
    eprintln!("MANIFEST:\n{out}\n");
    assert_eq!(out, original);
}

#[test]
fn allow_builds_clearing_legacy_is_a_noop_when_the_manifest_is_missing() {
    assert_eq!(run_allow_builds_clearing_legacy(None, &[]), None);
}

#[test]
fn remove_overrides_keeps_non_string_entries_of_a_flow_style_block() {
    // The decoded map cannot reserialize the non-string `bar`, but the flow
    // splice keeps its text, so the entry survives the removal of `foo`.
    let original = "overrides: { foo: link:../foo, bar: { nested: value } }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides: { bar: { nested: value } }\n");
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
fn delete_last_field_leaves_no_trailing_blank_line() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "cacheDir: ~/cache\n");
}

#[test]
fn delete_last_field_keeps_a_kept_chomped_block_scalars_trailing_blank() {
    for header in [
        "|+",
        ">+",
        "|+2",
        "|2+",
        "|2+ # keep the breaks",
        "|+ # retain > blanks",
        "&notes |+",
        "!!str >+",
    ] {
        let original = format!("notes: {header}\n  foo\n\nvirtualStoreDir: .pnpm\n");
        let out = run_update_field(Some(&original), "virtualStoreDir", &serde_json::Value::Null)
            .expect("file kept");
        assert_eq!(out, format!("notes: {header}\n  foo\n\n"), "header {header}");
    }
}

#[test]
fn delete_last_field_keeps_the_blank_below_a_deeper_indented_scalar_line() {
    let out = run_update_field(
        Some("notes: |+\n  foo\n    bar\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "notes: |+\n  foo\n    bar\n\n");
}

#[test]
fn delete_last_field_keeps_the_blank_of_a_scalar_under_a_quoted_key() {
    let out = run_update_field(
        Some("\"notes: title\": |+\n  foo\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "\"notes: title\": |+\n  foo\n\n");
}

#[test]
fn delete_last_field_keeps_the_blank_of_a_scalar_under_an_apostrophe_key() {
    for key in ["it's", "'it''s: title'"] {
        let original = format!("{key}: |+\n  foo\n\nvirtualStoreDir: .pnpm\n");
        let out = run_update_field(Some(&original), "virtualStoreDir", &serde_json::Value::Null)
            .expect("file kept");
        assert_eq!(out, format!("{key}: |+\n  foo\n\n"), "key {key}");
    }
}

#[test]
fn delete_last_field_drops_a_separator_below_a_header_written_in_a_comment() {
    for line in ["notes: text # detail: |+", "notes: text\n# detail: |+"] {
        let original = format!("{line}\n\nvirtualStoreDir: .pnpm\n");
        let out = run_update_field(Some(&original), "virtualStoreDir", &serde_json::Value::Null)
            .expect("file kept");
        assert_eq!(out, format!("{line}\n"), "line {line}");
    }
}

#[test]
fn delete_last_field_drops_a_separator_below_a_quoted_scalar_holding_a_header() {
    let out = run_update_field(
        Some("notes: \"foo |+ #\"\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "notes: \"foo |+ #\"\n");
}

#[test]
fn delete_last_field_drops_a_separator_below_an_unrelated_kept_chomped_scalar() {
    let out = run_update_field(
        Some("notes: |+\n  foo\n\nstoreDir: ~/store\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "notes: |+\n  foo\n\nstoreDir: ~/store\n");
}

#[test]
fn setting_a_field_after_deleting_the_last_one_keeps_a_single_blank_line() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "cacheDir: ~/cache\n\nstoreDir: ~/store\n";
    fs::write(&path, original).expect("seed manifest");
    let with_field = format!("{original}\nvirtualStoreDir: .pnpm\n");

    for value in [serde_json::json!(".pnpm"), serde_json::Value::Null, serde_json::json!(".pnpm")] {
        crate::update_manifest_field(&path, "virtualStoreDir", &value).expect("update succeeds");
    }

    assert_eq!(fs::read_to_string(&path).expect("file kept"), with_field);
}

#[test]
fn changing_the_value_of_the_last_field_keeps_its_single_blank_line() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nstoreDir: ~/store\n"),
        "storeDir",
        &serde_json::json!("~/other"),
    )
    .expect("file written");
    assert_eq!(out, "cacheDir: ~/cache\n\nstoreDir: ~/other\n");
}

#[test]
fn a_manifest_already_ending_in_a_blank_line_gains_no_second_one() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nstoreDir: ~/store\n\n"),
        "virtualStoreDir",
        &serde_json::json!(".pnpm"),
    )
    .expect("file written");
    assert_eq!(out, "cacheDir: ~/cache\n\nstoreDir: ~/store\n\nvirtualStoreDir: .pnpm\n");
}

#[test]
fn a_manifest_ending_in_a_whitespace_only_line_gains_no_second_blank() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nstoreDir: ~/store\n  \n"),
        "virtualStoreDir",
        &serde_json::json!(".pnpm"),
    )
    .expect("file written");
    assert_eq!(out, "cacheDir: ~/cache\n\nstoreDir: ~/store\n  \nvirtualStoreDir: .pnpm\n");
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

#[test]
fn delete_field_without_a_manifest_is_noop() {
    let out = run_update_field(None, "virtualStoreDir", &serde_json::Value::Null);
    assert_eq!(out, None);
}

/// Ported from upstream `removeCatalogs.test.ts`. The upstream tests
/// assert the parsed shape; these assert the format-preserving text the
/// pacquet writer produces for the same inputs.
mod remove_unused_catalogs {
    use super::{PackageManifest, catalogs, project, run_cleanup};

    /// TS: `remove the default catalog if it is empty`.
    #[test]
    fn removes_the_default_catalog_when_nothing_references_it() {
        let consumer = project(serde_json::json!({ "dependencies": { "foo": "^0.1.2" } }));
        let out = run_cleanup(Some("catalog:\n  foo: ^0.1.2\n"), None, &[&consumer]);
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    /// TS: `remove the unused default catalog`.
    #[test]
    fn removes_the_unused_default_catalog_entries() {
        let consumer = project(serde_json::json!({
            "dependencies": { "foo": "^0.1.2", "bar": "catalog:" },
        }));
        let out = run_cleanup(Some("catalog:\n  bar: 3.2.1\n  foo: ^0.1.2\n"), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some("catalog:\n  bar: 3.2.1\n"));
    }

    /// TS: `remove the unused default catalog with catalogs`.
    #[test]
    fn removes_the_unused_entries_under_catalogs_default() {
        let consumer = project(serde_json::json!({
            "dependencies": { "foo": "^0.1.2", "bar": "catalog:" },
        }));
        let out = run_cleanup(
            Some("catalogs:\n  default:\n    bar: 3.2.1\n    foo: ^0.1.2\n"),
            None,
            &[&consumer],
        );
        assert_eq!(out.as_deref(), Some("catalogs:\n  default:\n    bar: 3.2.1\n"));
    }

    /// TS: `remove the unused named catalog`.
    #[test]
    fn removes_an_entirely_unused_named_catalog() {
        let consumer = project(serde_json::json!({
            "dependencies": { "abc": "0.1.2", "def": "catalog:bar" },
        }));
        let out = run_cleanup(
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n  bar:\n    def: 3.2.1\n"),
            None,
            &[&consumer],
        );
        assert_eq!(out.as_deref(), Some("catalogs:\n  bar:\n    def: 3.2.1\n"));
    }

    /// TS: `remove all unused named catalogs` (the final cleanup that
    /// empties every named catalog and deletes the file).
    #[test]
    fn removes_the_file_when_every_named_catalog_empties() {
        let consumer = project(serde_json::json!({ "dependencies": { "def": "3.2.1" } }));
        let out = run_cleanup(
            Some("catalogs:\n  bar:\n    def: 3.2.1\n  foo:\n    ghi: 7.8.9\n"),
            None,
            &[&consumer],
        );
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    /// TS: `same pkg with different version` — the same package name may
    /// be referenced per catalog; each referenced entry survives.
    #[test]
    fn keeps_the_same_package_in_each_referenced_catalog() {
        let consumer = project(serde_json::json!({
            "dependencies": { "def": "catalog:bar", "ghi": "catalog:foo", "abc": "catalog:foo" },
            "optionalDependencies": { "abc": "catalog:bar" },
        }));
        let original = "catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n  bar:\n    abc: 1.2.3\n    def: 3.2.1\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some(original));
    }

    /// TS: `update catalogs and remove catalog` — one write both merges
    /// updated entries and drops the unreferenced ones.
    #[test]
    fn updates_catalogs_and_cleans_up_in_one_write() {
        let consumer = project(serde_json::json!({
            "dependencies": { "def": "catalog:bar", "ghi": "catalog:foo" },
        }));
        let out = run_cleanup(
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n  bar:\n    def: 3.2.1\n"),
            Some(&catalogs(&[("foo", &[("ghi", "7.9.9")])])),
            &[&consumer],
        );
        assert_eq!(
            out.as_deref(),
            Some("catalogs:\n  foo:\n    ghi: 7.9.9\n  bar:\n    def: 3.2.1\n"),
        );
    }

    /// TS: `when allProjects is undefined should not cleanup unused
    /// catalogs`.
    #[test]
    fn skips_cleanup_without_projects() {
        let projects: [&PackageManifest; 0] = [];
        let out = run_cleanup(
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n  bar:\n    def: 3.2.1\n"),
            Some(&catalogs(&[("foo", &[("ghi", "7.9.9")])])),
            &projects,
        );
        assert_eq!(
            out.as_deref(),
            Some("catalogs:\n  foo:\n    abc: 0.1.2\n    ghi: 7.9.9\n  bar:\n    def: 3.2.1\n"),
        );
    }

    #[test]
    fn prunes_a_flow_style_catalogs_mapping() {
        let consumer = project(serde_json::json!({ "dependencies": { "def": "catalog:bar" } }));
        let original = "catalogs: { foo: { abc: 0.1.2 }, bar: { def: 3.2.1 } }\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some("catalogs: { bar: { def: 3.2.1 } }\n"));
    }

    #[test]
    fn prunes_the_entries_of_a_flow_style_named_catalog() {
        let consumer = project(serde_json::json!({ "dependencies": { "abc": "catalog:foo" } }));
        let original = "catalogs: { foo: { abc: 0.1.2, ghi: 7.8.9 } }\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some("catalogs: { foo: { abc: 0.1.2 } }\n"));
    }

    /// TS: `keep catalogs referenced only in workspace overrides`.
    #[test]
    fn keeps_entries_referenced_only_by_workspace_overrides() {
        let consumer = project(serde_json::json!({ "dependencies": { "zoo": "^1.0.0" } }));
        let original = "catalog:\n  foo: 1.0.0\n\
            catalogs:\n  bar:\n    '@scope/def': 2.0.0\n\
            overrides:\n  foo: 'catalog:'\n  '@scope/parent@1>@scope/def': 'catalog:bar'\n";
        let out = run_cleanup(Some(original), None, &[&consumer]);
        assert_eq!(out.as_deref(), Some(original));
    }

    /// TS: `remove catalogs unused by dependencies and workspace
    /// overrides`.
    #[test]
    fn removes_entries_unreferenced_by_dependencies_and_overrides() {
        let consumer = project(serde_json::json!({ "dependencies": { "zoo": "^1.0.0" } }));
        let out = run_cleanup(
            Some(
                "catalog:\n  foo: 1.0.0\n  unusedDefault: 2.0.0\n\
                 catalogs:\n  bar:\n    def: 2.0.0\n    unusedNamed: 3.0.0\n\
                 overrides:\n  foo: 'catalog:'\n  def: 'catalog:bar'\n",
            ),
            None,
            &[&consumer],
        );
        assert_eq!(
            out.as_deref(),
            Some(
                "catalog:\n  foo: 1.0.0\n\
                 catalogs:\n  bar:\n    def: 2.0.0\n\
                 overrides:\n  foo: 'catalog:'\n  def: 'catalog:bar'\n",
            ),
        );
    }
}

/// The `minimumReleaseAgeExcludePrune` pass: entries of
/// `minimumReleaseAgeExclude` are pruned against the versions the
/// freshly resolved lockfile records.
mod minimum_release_age_exclude_prune {
    use crate::ResolvedPackageVersions;

    use super::{UpdateWorkspaceManifestOptions, run_with};

    fn resolved(entries: &[(&str, &[&str])]) -> ResolvedPackageVersions {
        entries
            .iter()
            .map(|(name, versions)| {
                (name.to_string(), versions.iter().map(ToString::to_string).collect())
            })
            .collect()
    }

    fn run_age_cleanup(
        original: Option<&str>,
        resolved: Option<&ResolvedPackageVersions>,
    ) -> Option<String> {
        run_with(
            original,
            &UpdateWorkspaceManifestOptions {
                prune_minimum_release_age_excludes: true,
                resolved_package_versions: resolved,
                ..Default::default()
            },
        )
    }

    #[test]
    fn drops_a_versioned_entry_whose_version_is_no_longer_resolved() {
        let original = "packages:\n  - '*'\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("packages:\n  - '*'\n"));
    }

    #[test]
    fn keeps_a_versioned_entry_whose_version_is_resolved() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn rewrites_a_narrowed_version_union_canonically() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0 || 2.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - foo@2.0.0\n"));
    }

    #[test]
    fn keeps_a_union_entry_verbatim_when_every_version_is_resolved() {
        let original = "minimumReleaseAgeExclude:\n  - foo@2.0.0 || 1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["1.0.0", "2.0.0"])])));
        assert_eq!(out.as_deref(), Some(original), "no version was dropped, so no rewrite");
    }

    #[test]
    fn keeps_a_bare_name_when_the_package_is_resolved() {
        let original = "minimumReleaseAgeExclude:\n  - foo\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &["2.0.0"])])));
        assert_eq!(out.as_deref(), Some(original));
    }

    /// A package resolved only via a non-semver source (git, tarball,
    /// `file:`) registers with an empty version set: its bare-name entry
    /// survives (the package is still a dependency) but its versioned
    /// entries are pruned (no exact version can be confirmed).
    #[test]
    fn keeps_the_bare_name_but_prunes_versions_of_a_non_semver_only_package() {
        let original = "minimumReleaseAgeExclude:\n  - foo\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("foo", &[])])));
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - foo\n"));
    }

    #[test]
    fn drops_a_bare_name_when_the_package_is_absent() {
        let original = "minimumReleaseAgeExclude:\n  - foo\n  - bar@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[("bar", &["1.0.0"])])));
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - bar@1.0.0\n"));
    }

    #[test]
    fn keeps_a_glob_entry_with_no_match() {
        let original = "minimumReleaseAgeExclude:\n  - '@babel/*'\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn removes_the_file_when_the_emptied_block_was_the_only_key() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out, None, "an emptied manifest file must be deleted");
    }

    #[test]
    fn skips_cleanup_without_resolved_versions() {
        let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
        let out = run_age_cleanup(Some(original), None);
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn keeps_an_unparsable_entry_verbatim() {
        let original = "minimumReleaseAgeExclude:\n  - foo@>=1.0.0\n";
        let out = run_age_cleanup(Some(original), Some(&resolved(&[])));
        assert_eq!(out.as_deref(), Some(original));
    }
}

/// pnpm scaffolds undecided entries with a multi-word plain scalar.
/// Deciding one replaces the whole value: ending it at the first
/// whitespace would leave `true this to true or false` behind, which
/// YAML reads as a string, so the package would stay undecided.
#[test]
fn allow_builds_replaces_a_multi_word_placeholder_value() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: set this to true or false\n"),
        &[("esbuild", true)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true\n"));
}

/// A comment after the value is the one thing that must survive the
/// replacement, which is why the value span stops at ` #`.
#[test]
fn allow_builds_keeps_a_trailing_comment() {
    let out =
        run_allow_builds(Some("allowBuilds:\n  esbuild: false # why\n"), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true # why\n"));
}

/// A `#` inside a quoted value is part of the value, not a comment, so
/// the replacement must not preserve it as one.
#[test]
fn allow_builds_replaces_a_quoted_value_containing_a_hash() {
    let out = run_allow_builds(Some("allowBuilds:\n  esbuild: \"a # b\"\n"), &[("esbuild", false)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: false\n"));
}

/// A quote only delimits a scalar when it opens the value, so an
/// apostrophe inside a plain scalar is a character, not an unterminated
/// quoted string that would swallow a following comment.
#[test]
fn allow_builds_replaces_a_plain_value_containing_a_quote() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: don't know yet # decide later\n"),
        &[("esbuild", true)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true # decide later\n"));
}

/// An escaped quote does not end a double-quoted scalar, so a `#` after
/// it is still inside the value.
#[test]
fn allow_builds_replaces_a_value_with_an_escaped_quote() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: \"a \\\" # b\" # real\n"),
        &[("esbuild", false)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: false # real\n"));
}

/// A doubled quote is the single-quoted style's escape, so it does not
/// end the scalar either.
#[test]
fn allow_builds_replaces_a_value_with_a_doubled_single_quote() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: 'it''s # fine' # real\n"),
        &[("esbuild", true)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true # real\n"));
}

fn run_prune_allow_builds(original: Option<&str>, resolved: &[&str]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("pnpm-workspace.yaml");
    if let Some(text) = original {
        std::fs::write(&path, text).expect("write manifest");
    }
    let mut resolved_map = std::collections::BTreeMap::new();
    for name in resolved {
        resolved_map.insert(name.to_string(), std::collections::BTreeSet::new());
    }
    crate::update_workspace_manifest(
        dir.path(),
        &crate::UpdateWorkspaceManifestOptions {
            prune_allow_builds: true,
            resolved_package_versions: Some(&resolved_map),
            ..Default::default()
        },
    )
    .expect("update succeeds");
    path.exists().then(|| std::fs::read_to_string(&path).expect("read manifest"))
}

#[test]
fn prune_allow_builds_removes_undecided_entry_whose_package_is_not_resolved() {
    let original =
        "allowBuilds:\n  foo: set this to true or false\n  bar: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &["foo"]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  foo: set this to true or false\n"));
}

#[test]
fn prune_allow_builds_keeps_decided_entries() {
    let original = "allowBuilds:\n  foo: true\n  bar: false\n  baz: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  foo: true\n  bar: false\n"));
}

#[test]
fn prune_allow_builds_deletes_block_and_file_when_empty() {
    let original = "allowBuilds:\n  foo: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out, None);
}

#[test]
fn prune_allow_builds_edits_a_flow_mapping_in_place() {
    let original = "allowBuilds: {foo: true, bar: set this to true or false}\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some("allowBuilds: { foo: true }\n"));
}

#[test]
fn prune_allow_builds_keeps_a_flow_mapping_comment_and_quoting() {
    let original = "allowBuilds: {foo: 'set this to true or false', bar: true, baz: 'set this to true or false'} # hey\n";
    let out = run_prune_allow_builds(Some(original), &["foo"]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds: { foo: 'set this to true or false', bar: true } # hey\n"),
    );
}

#[test]
fn prune_allow_builds_prunes_a_dep_path_key_by_its_package_name() {
    let original = "allowBuilds:\n  \
         foo@git+https://github.com/org/foo.git#0000000000000000000000000000000000000000: set this to true or false\n  \
         bar@git+https://github.com/org/bar.git#0000000000000000000000000000000000000000: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &["foo"]);
    assert_eq!(
        out.as_deref(),
        Some(
            "allowBuilds:\n  foo@git+https://github.com/org/foo.git#0000000000000000000000000000000000000000: set this to true or false\n"
        ),
    );
}

#[test]
fn prune_allow_builds_keeps_keys_with_no_provable_package_name() {
    let original =
        "allowBuilds:\n  foo@git+https://github.com/org/foo.git: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some(original));
}

#[test]
fn prune_allow_builds_prunes_an_escaped_quoted_key() {
    let original = "allowBuilds:\n  \"\\u0066oo\": set this to true or false\n  bar: true\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  bar: true\n"));
}

/// Every writer edits a hand-written single-line flow collection in place,
/// matching what the TypeScript writer's yaml library emits, and refuses a
/// multi-line one rather than dropping the comments between its entries.
mod flow_style {
    use super::{
        TempDir, UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, catalogs, fs, run,
        run_age_excludes, run_allow_builds, run_config_dep, run_ignore_ghsas, run_patched_deps,
        run_remove_overrides, run_scaffold_allow_builds, update_workspace_manifest,
    };

    #[test]
    fn catalog_entry_is_added_to_a_flow_mapping() {
        let out = run(
            Some("catalog: { foo: ^1.0.0 }\n"),
            &catalogs(&[("default", &[("bar", "^2.0.0")])]),
        );
        assert_eq!(out.as_deref(), Some("catalog: { bar: ^2.0.0, foo: ^1.0.0 }\n"));
    }

    #[test]
    fn catalog_entry_is_updated_in_a_flow_mapping() {
        let out = run(
            Some("catalog: { foo: ^1.0.0 } # pins\n"),
            &catalogs(&[("default", &[("foo", "^2.0.0")])]),
        );
        assert_eq!(out.as_deref(), Some("catalog: { foo: ^2.0.0 } # pins\n"));
    }

    #[test]
    fn named_catalog_entry_is_added_to_a_nested_flow_mapping() {
        let out = run(
            Some("catalogs: { myCatalog: { foo: ^1.0.0 } }\n"),
            &catalogs(&[("myCatalog", &[("bar", "^2.0.0")])]),
        );
        assert_eq!(out.as_deref(), Some("catalogs: { myCatalog: { bar: ^2.0.0, foo: ^1.0.0 } }\n"));
    }

    #[test]
    fn a_new_named_catalog_is_added_to_a_flow_catalogs_mapping() {
        let out = run(
            Some("catalogs: { myCatalog: { foo: ^1.0.0 } }\n"),
            &catalogs(&[("newCatalog", &[("bar", "^2.0.0")])]),
        );
        assert_eq!(
            out.as_deref(),
            Some("catalogs: { myCatalog: { foo: ^1.0.0 }, newCatalog: { bar: ^2.0.0 } }\n"),
        );
    }

    #[test]
    fn config_dependency_is_added_to_a_flow_mapping() {
        let out = run_config_dep(Some("configDependencies: { foo: 1.0.0 }\n"), "bar", "2.0.0");
        assert_eq!(out, "configDependencies: { bar: 2.0.0, foo: 1.0.0 }\n");
    }

    #[test]
    fn allow_build_is_added_to_a_flow_mapping() {
        let out = run_allow_builds(Some("allowBuilds: { foo: true }\n"), &[("bar", false)]);
        assert_eq!(out.as_deref(), Some("allowBuilds: { bar: false, foo: true }\n"));
    }

    #[test]
    fn allow_build_is_updated_in_a_flow_mapping() {
        let out = run_allow_builds(Some("allowBuilds: { foo: true }\n"), &[("foo", false)]);
        assert_eq!(out.as_deref(), Some("allowBuilds: { foo: false }\n"));
    }

    #[test]
    fn undecided_allow_build_is_added_to_a_flow_mapping() {
        let out = run_scaffold_allow_builds(Some("allowBuilds: { foo: true }\n"), &["bar"]);
        assert_eq!(
            out.as_deref(),
            Some("allowBuilds: { bar: set this to true or false, foo: true }\n"),
        );
    }

    #[test]
    fn undecided_allow_build_leaves_a_decided_flow_entry_alone() {
        let original = "allowBuilds: { foo: true }\n";
        let out = run_scaffold_allow_builds(Some(original), &["foo"]);
        assert_eq!(out.as_deref(), Some(original));
    }

    #[test]
    fn patched_dependency_is_added_to_a_flow_mapping() {
        let out = run_patched_deps(
            Some("patchedDependencies: { foo: patches/foo.patch }\n"),
            &[("foo", "patches/foo.patch"), ("bar", "patches/bar.patch")],
        );
        assert_eq!(
            out,
            "patchedDependencies: { bar: patches/bar.patch, foo: patches/foo.patch }\n",
        );
    }

    #[test]
    fn omitted_patched_dependency_is_dropped_from_a_flow_mapping() {
        let out = run_patched_deps(
            Some("patchedDependencies: { foo: patches/foo.patch, bar: patches/bar.patch }\n"),
            &[("bar", "patches/bar.patch")],
        );
        assert_eq!(out, "patchedDependencies: { bar: patches/bar.patch }\n");
    }

    #[test]
    fn minimum_release_age_excludes_stay_a_flow_sequence() {
        let out = run_age_excludes(
            Some("minimumReleaseAgeExclude: [foo@1.0.0]\n"),
            &["foo@1.0.0", "bar@2.0.0"],
        );
        assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude: [ foo@1.0.0, bar@2.0.0 ]\n"));
    }

    #[test]
    fn ignore_ghsas_stay_a_flow_sequence_under_a_block_audit_config() {
        let out = run_ignore_ghsas(
            Some("auditConfig:\n  ignoreGhsas: [GHSA-aaaa-bbbb-cccc, GHSA-dddd-eeee-ffff]\n"),
            &["GHSA-gggg-hhhh-iiii"],
        );
        assert_eq!(out.as_deref(), Some("auditConfig:\n  ignoreGhsas: [ GHSA-gggg-hhhh-iiii ]\n"));
    }

    #[test]
    fn ignore_ghsas_are_added_to_a_flow_audit_config() {
        let out = run_ignore_ghsas(Some("auditConfig: {}\n"), &["GHSA-aaaa-bbbb-cccc"]);
        assert_eq!(out.as_deref(), Some("auditConfig: { ignoreGhsas: [ GHSA-aaaa-bbbb-cccc ] }\n"));
    }

    #[test]
    fn a_multiline_flow_mapping_is_refused_rather_than_flattened() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "allowBuilds: {\n  foo: true, # decided\n}\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_allow_builds(dir.path(), [("bar", true)])
            .expect_err("must refuse a multi-line inline allowBuilds block");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn a_multiline_flow_sequence_is_refused_rather_than_rewritten() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "minimumReleaseAgeExclude: [\n  foo@1.0.0, # pinned\n  bar@2.0.0,\n]\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_minimum_release_age_excludes(dir.path(), &["baz@3.0.0".to_string()])
            .expect_err("must refuse a multi-line inline sequence");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn a_multiline_flow_block_is_dropped_whole_when_it_empties() {
        let original = "packages:\n  - '*'\noverrides: {\n  foo: link:../foo, # pinned\n}\n";
        let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
        assert_eq!(out, "packages:\n  - '*'\n");
    }

    #[test]
    fn a_multiline_flow_block_is_deleted_whole_by_config_delete() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        fs::write(&path, "overrides: {\n  foo: 1.0.0, # pinned\n}\npackages:\n  - '*'\n")
            .expect("seed manifest");

        crate::update_manifest_field(&path, "overrides", &serde_json::Value::Null)
            .expect("update_manifest_field succeeds");

        assert_eq!(fs::read_to_string(&path).expect("read manifest"), "packages:\n  - '*'\n");
    }

    /// A block whose value has the wrong shape for its setting never reaches
    /// the writers: the typed parse rejects the manifest first.
    #[test]
    fn a_flow_collection_of_the_wrong_kind_fails_to_parse() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "allowBuilds: [ foo ]\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_allow_builds(dir.path(), [("bar", true)])
            .expect_err("must refuse a sequence where allowBuilds expects a mapping");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::Parse { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn a_whole_document_flow_mapping_is_refused_rather_than_corrupted() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        // The keys of a document written as one flow mapping are not
        // top-level lines, so no splice — nor a new top-level block — can
        // address them.
        let original = "{ overrides: { foo: 1.0.0 } }\n";
        fs::write(&path, original).expect("seed manifest");

        let err = crate::set_overrides(dir.path(), [("bar", "2.0.0")])
            .expect_err("must refuse a whole-document flow mapping");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }

    #[test]
    fn an_aliased_block_is_refused_rather_than_corrupted() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
        let original = "catalog: &pins { foo: ^1.0.0 }\ncatalogs: { other: *pins }\n";
        fs::write(&path, original).expect("seed manifest");

        let err = update_workspace_manifest(
            dir.path(),
            &UpdateWorkspaceManifestOptions {
                updated_catalogs: Some(&catalogs(&[("other", &[("bar", "^2.0.0")])])),
                ..Default::default()
            },
        )
        .expect_err("must refuse an aliased catalog block");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
        assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
    }
}
