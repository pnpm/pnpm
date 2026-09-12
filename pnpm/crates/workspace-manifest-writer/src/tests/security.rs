use super::{run_allow_builds, run_patched_deps, run_prune_allow_builds};

#[cfg(unix)]
use super::{TempDir, WORKSPACE_MANIFEST_FILENAME, fs};

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
fn patched_dependency_removes_empty_or_null_block() {
    let empty = "packages:\n  - '*'\n\npatchedDependencies:\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(empty), &[]);
    assert_eq!(out, "packages:\n  - '*'\n\ncatalog:\n  react: 18.2.0\n");

    let null = "packages:\n  - '*'\n\npatchedDependencies: null\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(null), &[]);
    assert_eq!(out, "packages:\n  - '*'\n\ncatalog:\n  react: 18.2.0\n");
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

#[test]
fn prune_allow_builds_prunes_an_escaped_quoted_key() {
    let original = "allowBuilds:\n  \"\\u0066oo\": set this to true or false\n  bar: true\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  bar: true\n"));
}
