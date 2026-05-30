//! End-to-end integration tests for [`DirectoryFetcher::run`]. The
//! per-walker invariants live alongside `walker.rs`; this file
//! exercises the full request/response shape upstream callers depend
//! on (`manifest`, `requires_build`, `files_map` keys).

use pacquet_directory_fetcher::DirectoryFetcher;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::tempdir;

fn touch(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

#[test]
fn run_in_all_files_mode_returns_manifest_and_filesmap() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json", r#"{ "name": "x", "version": "0.0.0" }"#);
    touch(root, "src/index.ts", "");
    touch(root, "node_modules/foo/index.js", "");

    let out = DirectoryFetcher {
        directory: root.to_path_buf(),
        include_only_package_files: false,
        resolve_symlinks: false,
    }
    .run()
    .unwrap();

    // node_modules dropped; manifest read; no install scripts ↔
    // requires_build = false.
    let mut rels: Vec<_> = out.files_map.keys().cloned().collect();
    rels.sort();
    assert_eq!(rels, vec!["package.json".to_string(), "src/index.ts".into()]);

    let manifest = out.manifest.expect("manifest read");
    assert_eq!(manifest.get("name").and_then(|v| v.as_str()), Some("x"));
    assert!(!out.requires_build);
}

#[test]
fn run_flags_requires_build_when_install_script_present() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(
        root,
        "package.json",
        r#"{
            "name": "needs-build",
            "version": "0.0.0",
            "scripts": { "install": "node-gyp rebuild" }
        }"#,
    );

    let out = DirectoryFetcher {
        directory: root.to_path_buf(),
        include_only_package_files: false,
        resolve_symlinks: false,
    }
    .run()
    .unwrap();

    // `pkg_requires_build` sees the install script in the manifest
    // and flips the bit. Matches upstream's
    // `pkgRequiresBuild(manifest, filesIndex)` at
    // <https://github.com/pnpm/pnpm/blob/85ceff2383/building/pkg-requires-build/src/index.ts>.
    assert!(out.requires_build);
}

#[test]
fn run_flags_requires_build_when_binding_gyp_present() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json", r#"{ "name": "native", "version": "0.0.0" }"#);
    touch(root, "binding.gyp", "{ 'targets': [] }");

    let out = DirectoryFetcher {
        directory: root.to_path_buf(),
        include_only_package_files: false,
        resolve_symlinks: false,
    }
    .run()
    .unwrap();

    // No install script in the manifest, but `binding.gyp` at the
    // package root is the canonical "this is a node-gyp native
    // module" signal. Mirrors upstream's `filesIncludeInstallScripts`
    // check.
    assert!(out.requires_build);
}

#[test]
fn run_returns_none_manifest_for_bit_workspace_directory_without_package_json() {
    // pnpm's `safeReadProjectManifestOnly` returns null when no
    // manifest variant exists; pacquet's `safe_read_package_json_from_dir`
    // returns `Ok(None)` for the same `ENOENT` case. The fetcher
    // must surface that as `manifest: None` rather than erroring —
    // the Bit-workspace shape upstream documents at
    // <https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L63-L66>.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "index.js", "");

    let out = DirectoryFetcher {
        directory: root.to_path_buf(),
        include_only_package_files: false,
        resolve_symlinks: false,
    }
    .run()
    .unwrap();

    assert!(out.manifest.is_none());
    assert!(!out.requires_build);
    assert_eq!(out.files_map.len(), 1);
}

#[test]
fn run_in_package_files_mode_honors_files_field() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json", r#"{ "name": "x", "version": "0.0.0", "files": ["dist/**"] }"#);
    touch(root, "dist/index.js", "");
    touch(root, "src/internal.ts", "");

    let out = DirectoryFetcher {
        directory: root.to_path_buf(),
        include_only_package_files: true,
        resolve_symlinks: false,
    }
    .run()
    .unwrap();

    let mut rels: Vec<_> = out.files_map.keys().cloned().collect();
    rels.sort();

    assert_eq!(rels, vec!["dist/index.js".to_string(), "package.json".into()]);
}
