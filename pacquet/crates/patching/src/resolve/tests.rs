use crate::resolve::{ResolvePatchedDependenciesError, resolve_and_group};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;

const HELLO_SHA256_HEX: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

fn raw(entries: &[(&str, &str)]) -> IndexMap<String, String> {
    entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn empty_input_returns_none() {
    let dir = tempdir().unwrap();
    let result = resolve_and_group(dir.path(), &IndexMap::new()).unwrap();
    assert_eq!(result, None);
}

#[test]
fn resolves_relative_paths_against_workspace_dir() {
    let workspace = tempdir().unwrap();
    let patches = workspace.path().join("patches");
    fs::create_dir(&patches).unwrap();
    fs::write(patches.join("lodash@4.17.21.patch"), b"hello\n").unwrap();

    let input = raw(&[("lodash@4.17.21", "patches/lodash@4.17.21.patch")]);
    let groups = resolve_and_group(workspace.path(), &input).unwrap().unwrap();

    let lodash = groups.get("lodash").expect("lodash group");
    let exact = lodash.exact.get("4.17.21").expect("exact 4.17.21 present");
    assert_eq!(exact.hash, HELLO_SHA256_HEX);
    assert_eq!(exact.key, "lodash@4.17.21");
    assert_eq!(
        exact.patch_file_path.as_deref(),
        Some(patches.join("lodash@4.17.21.patch")).as_deref(),
    );
}

#[test]
fn absolute_paths_are_used_verbatim() {
    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let patch_file = outside.path().join("absolute.patch");
    fs::write(&patch_file, b"hello\n").unwrap();

    let input = raw(&[("foo@1.0.0", &patch_file.display().to_string())]);
    let groups = resolve_and_group(workspace.path(), &input).unwrap().unwrap();

    let foo = groups.get("foo").expect("foo group");
    let exact = foo.exact.get("1.0.0").expect("exact 1.0.0 present");
    assert_eq!(exact.patch_file_path.as_deref(), Some(patch_file.as_path()));
    assert_eq!(exact.hash, HELLO_SHA256_HEX);
}

#[test]
fn nonexistent_patch_file_errors() {
    let workspace = tempdir().unwrap();
    let input = raw(&[("foo@1.0.0", "patches/missing.patch")]);
    let err = resolve_and_group(workspace.path(), &input).unwrap_err();
    assert!(matches!(err, ResolvePatchedDependenciesError::Hash(_)), "got: {err:?}");
}

#[test]
fn invalid_version_range_propagates() {
    let workspace = tempdir().unwrap();
    let patches = workspace.path().join("patches");
    fs::create_dir(&patches).unwrap();
    fs::write(patches.join("foo.patch"), b"hello\n").unwrap();

    let input = raw(&[("foo@link:packages/foo", "patches/foo.patch")]);
    let err = resolve_and_group(workspace.path(), &input).unwrap_err();
    assert!(matches!(err, ResolvePatchedDependenciesError::Range(_)), "got: {err:?}");
}

/// Mixed entry types — exact version, range, bare wildcard — all
/// resolve and hash from the same workspace dir, then bucket via
/// `group_patched_dependencies`.
#[test]
fn mixed_entries_resolve_in_one_call() {
    let workspace = tempdir().unwrap();
    let patches = workspace.path().join("patches");
    fs::create_dir(&patches).unwrap();
    fs::write(patches.join("a.patch"), b"hello\n").unwrap();
    fs::write(patches.join("b.patch"), b"hello\n").unwrap();
    fs::write(patches.join("c.patch"), b"hello\n").unwrap();

    let input = raw(&[
        ("foo@1.0.0", "patches/a.patch"),
        ("foo@^2.0.0", "patches/b.patch"),
        ("bar", "patches/c.patch"),
    ]);

    let groups = resolve_and_group(workspace.path(), &input).unwrap().unwrap();
    let foo = groups.get("foo").expect("foo group");
    assert!(foo.exact.contains_key("1.0.0"), "missing exact 1.0.0 in foo group: {foo:?}");
    assert_eq!(foo.range.len(), 1);
    assert_eq!(foo.range[0].version, "^2.0.0");
    let bar = groups.get("bar").expect("bar group");
    assert!(bar.all.is_some(), "missing wildcard in bar group: {bar:?}");
}

/// `IndexMap` input preserves the user's listed order end-to-end into
/// `PatchGroup.range`. Switching back to `BTreeMap` (alphabetical)
/// would silently break parity with upstream's JS-object iteration
/// order and surface as different `PATCH_KEY_CONFLICT` diagnostics
/// when multiple ranges match a version.
#[test]
fn range_preserves_user_specified_order() {
    let workspace = tempdir().unwrap();
    let patches = workspace.path().join("patches");
    fs::create_dir(&patches).unwrap();
    fs::write(patches.join("a.patch"), b"hello\n").unwrap();
    fs::write(patches.join("b.patch"), b"hello\n").unwrap();
    fs::write(patches.join("c.patch"), b"hello\n").unwrap();

    // User-specified order: `~1.2.0`, then `4`, then `>=5 <6`.
    // Alphabetical order would be: `4`, `>=5 <6`, `~1.2.0` (because
    // ASCII `4` < `>` < `~`). The assertion below pins the IndexMap
    // path; a regression to BTreeMap reorders the range vec
    // alphabetically and would fail.
    let input = raw(&[
        ("foo@~1.2.0", "patches/a.patch"),
        ("foo@4", "patches/b.patch"),
        ("foo@>=5 <6", "patches/c.patch"),
    ]);

    let groups = resolve_and_group(workspace.path(), &input).unwrap().unwrap();
    let foo = groups.get("foo").expect("foo group");
    let versions: Vec<&str> = foo.range.iter().map(|range| range.version.as_str()).collect();
    assert_eq!(versions, vec!["~1.2.0", "4", ">=5 <6"]);
}
