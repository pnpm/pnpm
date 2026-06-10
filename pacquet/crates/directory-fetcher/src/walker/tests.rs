use super::super::walker::{walk_all_files, walk_package_files};
use pretty_assertions::assert_eq;
use std::{collections::BTreeMap, fs, path::Path};
use tempfile::tempdir;

fn touch(root: &Path, rel: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, "").unwrap();
}

/// Collapse `walk_*` output to a `BTreeMap<rel_path, absolute_path
/// rendered as forward slashes relative to root>` so assertions stay
/// readable and order-independent. The value lets us distinguish
/// "symlink left as-is" vs "symlink resolved" without baking the
/// tmpdir's path into the assertion.
fn collect_rels(
    root: &Path,
    files: std::collections::HashMap<String, std::path::PathBuf>,
) -> BTreeMap<String, String> {
    files
        .into_iter()
        .map(|(rel, abs)| {
            // Use `dunce::canonicalize` semantics indirectly: strip
            // the tmp root prefix off the absolute path and report
            // the remainder. That keeps assertions deterministic.
            let stripped = abs.strip_prefix(root).map_or_else(
                |_| abs.display().to_string(),
                |path| path.display().to_string().replace('\\', "/"),
            );
            (rel, stripped)
        })
        .collect()
}

#[test]
fn walk_all_files_recurses_and_returns_relative_paths() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "lib/inner.js");
    touch(root, "lib/nested/deep.js");

    let out = walk_all_files(root, false).unwrap();
    let rels: BTreeMap<_, _> = collect_rels(root, out);

    assert_eq!(
        rels.keys().cloned().collect::<Vec<_>>(),
        vec![
            "index.js".to_string(),
            "lib/inner.js".into(),
            "lib/nested/deep.js".into(),
            "package.json".into(),
        ],
    );
}

#[test]
fn walk_all_files_skips_node_modules_at_root_and_nested() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "index.js");
    touch(root, "node_modules/foo/index.js");
    touch(root, "lib/node_modules/bar/index.js");

    let out = walk_all_files(root, false).unwrap();
    let rels: BTreeMap<_, _> = collect_rels(root, out);

    // node_modules at any depth must drop out — pnpm's
    // `fetchAllFilesFromDir` filters by basename in every recursion.
    assert_eq!(rels.keys().cloned().collect::<Vec<_>>(), vec!["index.js".to_string()]);
}

#[test]
fn walk_all_files_on_empty_directory_returns_empty_map() {
    let dir = tempdir().unwrap();
    let out = walk_all_files(dir.path(), false).unwrap();
    assert!(out.is_empty());
}

#[cfg(unix)]
#[test]
fn walk_all_files_terminates_on_symlink_cycle() {
    use std::os::unix::fs::symlink;

    // A `loop -> .` symlink would, without the cycle guard, sink the
    // walker into infinite recursion until either ENAMETOOLONG fires
    // or the Rust stack runs out. The visited-set guard keys off
    // `fs::canonicalize`, so `root/loop` canonicalises to `root` and
    // the second recursion bails before reading any further. The
    // direct children of `root` (incl. `real.txt`) still land in the
    // output; nothing under `loop/` does, because the recursive call
    // returns immediately on the cycle.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "real.txt");
    symlink(root, root.join("loop")).unwrap();

    let out = walk_all_files(root, false).unwrap();
    let rels: BTreeMap<_, _> = collect_rels(root, out);

    assert!(rels.contains_key("real.txt"), "direct children must still be walked: {rels:?}");
    assert!(
        rels.keys().all(|key| !key.starts_with("loop/")),
        "cycle guard must short-circuit before any `loop/` descendant is recorded: {rels:?}",
    );
}

#[cfg(unix)]
#[test]
fn walk_all_files_skips_broken_symlink_without_resolve_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "real.txt");
    symlink(root.join("missing.txt"), root.join("dangling")).unwrap();

    let out = walk_all_files(root, false).unwrap();
    let rels: BTreeMap<_, _> = collect_rels(root, out);

    assert_eq!(rels.keys().cloned().collect::<Vec<_>>(), vec!["real.txt".to_string()]);
}

#[cfg(unix)]
#[test]
fn walk_all_files_skips_broken_symlink_with_resolve_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "real.txt");
    symlink(root.join("missing.txt"), root.join("dangling")).unwrap();

    let out = walk_all_files(root, true).unwrap();
    let rels: BTreeMap<_, _> = collect_rels(root, out);

    assert_eq!(rels.keys().cloned().collect::<Vec<_>>(), vec!["real.txt".to_string()]);
}

#[cfg(unix)]
#[test]
fn walk_all_files_resolves_symlinks_when_requested() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path();
    // Build a target file outside the package dir, then a symlink
    // *inside* the package dir pointing at it. Under `resolve_symlinks`
    // upstream's `realFileStat` returns the realpath as the source
    // entry — confirm we do the same.
    let outside = tempdir().unwrap();
    let target = outside.path().join("target.txt");
    fs::write(&target, b"hello").unwrap();
    symlink(&target, root.join("link.txt")).unwrap();

    let out = walk_all_files(root, true).unwrap();
    assert_eq!(out.len(), 1);
    let src = out.get("link.txt").expect("link.txt entry");
    // The source path must be the realpath, not the symlink path.
    assert_eq!(
        fs::canonicalize(src).unwrap(),
        fs::canonicalize(&target).unwrap(),
        "resolve_symlinks must return the realpath as the source",
    );
}

#[cfg(unix)]
#[test]
fn walk_all_files_keeps_symlink_path_without_resolve_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path();
    let outside = tempdir().unwrap();
    let target = outside.path().join("target.txt");
    fs::write(&target, b"hello").unwrap();
    symlink(&target, root.join("link.txt")).unwrap();

    let out = walk_all_files(root, false).unwrap();
    let src = out.get("link.txt").expect("link.txt entry");
    // Without resolve_symlinks the source path stays as the symlink
    // location inside the package dir — upstream's `fileStat` uses
    // `fs.stat`, which follows the link for type/size but returns the
    // path the caller handed in.
    assert_eq!(src, &root.join("link.txt"));
}

#[test]
fn walk_package_files_applies_npm_packlist_filter() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "x", "version": "0.0.0", "files": ["dist/**"] }"#,
    )
    .unwrap();
    touch(root, "dist/index.js");
    touch(root, "dist/sub/inner.js");
    touch(root, "src/internal.ts");

    let out = walk_package_files(root).unwrap();
    let mut rels: Vec<_> = out.keys().cloned().collect();
    rels.sort();

    // `package.json` is always-included; the `files` field is
    // honoured; `src/` falls out.
    assert_eq!(
        rels,
        vec!["dist/index.js".to_string(), "dist/sub/inner.js".into(), "package.json".into(),],
    );
}

#[test]
fn walk_package_files_works_without_a_manifest() {
    // pnpm's `fetchPackageFilesFromDir` reads the manifest with
    // `safeReadProjectManifestOnly` and tolerates `null` for the
    // Bit-workspace case where dirs have no package.json. The
    // packlist then runs over an empty manifest (no `files`, no
    // `bundleDependencies`) and returns "everything except cruft".
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "index.js");
    touch(root, ".DS_Store");

    let out = walk_package_files(root).unwrap();
    let rels: Vec<_> = out.keys().cloned().collect();

    assert_eq!(rels, vec!["index.js".to_string()]);
}
