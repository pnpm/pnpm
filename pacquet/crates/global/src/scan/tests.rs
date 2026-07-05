use super::{
    find_global_package, get_installed_bin_names, read_direct_dependency_aliases,
    read_installed_packages, scan_global_packages,
};
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("json path has a parent")).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

/// Populate `install_dir` as a global group holding a downloaded Node.js
/// runtime: the manifest stores it under `engines.runtime` (the shape the
/// manifest writer folds `node: runtime:<v>` into), and the runtime is
/// materialized under `node_modules/node` with a synthesized `bin`.
fn write_runtime_group(install_dir: &Path) {
    write_json(
        &install_dir.join("package.json"),
        &json!({
            "engines": {
                "runtime": { "name": "node", "version": "22.11.0", "onFail": "download" },
            },
        }),
    );
    write_json(
        &install_dir.join("node_modules/node/package.json"),
        &json!({ "name": "node", "version": "22.11.0", "bin": { "node": "bin/node" } }),
    );
}

#[test]
fn runtime_engines_are_reified_as_a_direct_dependency() {
    let tmp = TempDir::new().unwrap();
    write_runtime_group(tmp.path());

    assert_eq!(read_direct_dependency_aliases(tmp.path()), vec!["node".to_string()]);

    let pkgs = read_installed_packages(tmp.path());
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0].location, tmp.path().join("node_modules/node"));
    assert_eq!(pkgs[0].manifest.get("bin"), Some(&json!({ "node": "bin/node" })));
}

#[test]
fn engines_runtime_without_download_is_not_treated_as_installed() {
    // A group whose manifest merely declares an engine *check*
    // (`onFail: "warn"`) has not downloaded a runtime, so it must not be
    // reified into a dependency and mistaken for an installed runtime.
    let tmp = TempDir::new().unwrap();
    write_json(
        &tmp.path().join("package.json"),
        &json!({
            "engines": {
                "runtime": { "name": "node", "version": "22.11.0", "onFail": "warn" },
            },
        }),
    );

    assert!(read_direct_dependency_aliases(tmp.path()).is_empty());
    assert!(read_installed_packages(tmp.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn scan_finds_a_globally_installed_runtime() {
    let global_dir = TempDir::new().unwrap();
    let install_dir = global_dir.path().join("install-abc");
    write_runtime_group(&install_dir);
    std::os::unix::fs::symlink(&install_dir, global_dir.path().join("hashkey")).unwrap();

    let groups = scan_global_packages(global_dir.path()).unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0].has_alias("node"));
    assert_eq!(get_installed_bin_names(&groups[0]), vec!["node".to_string()]);

    assert!(find_global_package(global_dir.path(), "node").unwrap().is_some());
}
