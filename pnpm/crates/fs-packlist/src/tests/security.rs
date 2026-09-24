use super::{PacklistOptions, fs, json, packlist, packlist_with_options, tempdir, touch};

#[cfg(unix)]
use super::write;

#[test]
fn bundle_dependencies_rejects_path_traversal() {
    // Defense-in-depth: a malicious manifest with a `..` in
    // bundleDependencies must not let the fetcher read files outside
    // the package directory.
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    fs::create_dir_all(&root).unwrap();
    touch(&root, "package.json");
    // A sibling next to the package's would-be node_modules. If
    // path traversal worked, this directory would be reachable via
    // `node_modules/../escape`.
    let escape = dir.path().join("escape");
    fs::create_dir_all(&escape).unwrap();
    fs::write(escape.join("secret.txt"), "DO NOT EXFIL\n").unwrap();

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["../escape"],
    });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        !out.iter()
            .any(|path| path.contains("escape") || path.contains("secret")),
        "bundle name traversal must not leak files outside pkg_dir: {out:?}",
    );
}

#[cfg(unix)]
#[test]
fn bundle_dependency_symlink_escaping_pkg_dir_is_refused() {
    // Defense-in-depth: the bundle name is a single safe segment
    // (`is_safe_bundle_name` accepts it), but `node_modules/<name>` is
    // a symlink pointing outside the package. Resolving and walking it
    // would splice host files into the published set. The fetcher
    // imports untrusted git-hosted packages, so this must be refused.
    let dir = tempdir().unwrap();
    let root = dir.path().join("pkg");
    fs::create_dir_all(root.join("node_modules")).unwrap();
    touch(&root, "package.json");
    // A sibling directory outside the package, made to look like a real
    // package so the walk-up resolves it.
    let escape = dir.path().join("escape");
    fs::create_dir_all(&escape).unwrap();
    fs::write(escape.join("package.json"), r#"{"name":"evil","version":"1.0.0"}"#).unwrap();
    fs::write(escape.join("secret.txt"), "DO NOT EXFIL\n").unwrap();
    std::os::unix::fs::symlink(&escape, root.join("node_modules/evil")).unwrap();

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["evil"],
    });
    let out = packlist(&root, &manifest).unwrap();

    assert!(
        !out.iter()
            .any(|path| path.contains("secret")),
        "a node_modules symlink escaping pkg_dir must not leak host files: {out:?}",
    );
}

#[test]
fn bundle_dependency_symlink_to_workspace_package_is_included() {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    let root = workspace.join("packages/app");
    fs::create_dir_all(root.join("node_modules")).unwrap();
    touch(&root, "package.json");
    let bundled = workspace.join("packages/bundled");
    fs::create_dir_all(&bundled).unwrap();
    fs::write(bundled.join("package.json"), r#"{"name":"bundled","version":"1.0.0"}"#).unwrap();
    fs::write(bundled.join("index.js"), "module.exports = 42").unwrap();
    pnpm_fs::symlink_dir(&bundled, &root.join("node_modules/bundled")).unwrap();

    let manifest = json!({
        "name": "app",
        "version": "1.0.0",
        "bundleDependencies": ["bundled"],
    });
    let out = packlist_with_options(
        &root,
        &manifest,
        PacklistOptions { workspace_dir: Some(workspace), ..Default::default() },
    )
    .unwrap();

    assert!(out.contains(&"node_modules/bundled/index.js".to_string()));
}

/// An intermediate symlinked directory (`subdir -> /outside`) lets a
/// lexically-contained `main` resolve to a host file, so containment must
/// be verified against the canonical resolved path, not just the string.
#[cfg(unix)]
#[test]
fn main_resolving_through_a_symlinked_dir_is_not_force_included() {
    let outside = tempdir().unwrap();
    write(outside.path(), "secret.js", "SECRET");
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    std::os::unix::fs::symlink(outside.path(), root.join("subdir")).unwrap();

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "main": "subdir/secret.js",
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        !out.iter()
            .any(|path| path.contains("secret")),
        "main resolving outside the package via a symlinked dir must not be included: {out:?}",
    );
}
