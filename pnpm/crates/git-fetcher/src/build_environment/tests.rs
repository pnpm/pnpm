use super::{BuildEnvironment, detect_build_environment, requires_build};
use crate::{PreferredPm, WantedPm};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn requires_build_false_for_empty_scripts() {
    let dir = tempdir().unwrap();
    let manifest = json!({ "name": "x", "version": "0.0.0" });
    assert!(!requires_build(&manifest, dir.path()));
}

#[test]
fn requires_build_true_for_non_empty_prepare() {
    let dir = tempdir().unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "scripts": { "prepare": "tsc" },
    });
    assert!(requires_build(&manifest, dir.path()));
}

#[test]
fn requires_build_false_when_main_exists_and_prepare_absent() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.js"), "").unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "scripts": { "prepublish": "true" },
    });
    assert!(!requires_build(&manifest, dir.path()));
}

#[test]
fn requires_build_true_when_main_missing_and_prepublish_set() {
    let dir = tempdir().unwrap();
    // `main` defaults to `index.js`; create no file so it's missing.
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "scripts": { "prepack": "rollup -c" },
    });
    assert!(requires_build(&manifest, dir.path()));
}

#[test]
fn no_environment_for_a_package_without_a_build() {
    let dir = tempdir().unwrap();
    let manifest = json!({ "name": "x", "version": "0.0.0", "packageManager": "yarn@4.9.2" });
    assert_eq!(detect_build_environment(dir.path(), dir.path(), &manifest), None);
}

#[test]
fn environment_names_the_pinned_package_manager_and_node() {
    let dir = tempdir().unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "packageManager": "yarn@4.9.2",
        "devEngines": {
            "runtime": [
                { "name": "bun", "version": "1.2.0", "onFail": "download" },
                { "name": "node", "version": "^24.6.0", "onFail": "download" },
                { "name": "node", "version": "22.0.0", "onFail": "download" },
            ],
        },
        "scripts": { "prepare": "tsc" },
    });
    assert_eq!(
        detect_build_environment(dir.path(), dir.path(), &manifest),
        Some(BuildEnvironment {
            package_manager: WantedPm {
                pm: PreferredPm::Yarn,
                version_spec: Some("4.9.2".to_string()),
                pinned: true,
            },
            node: Some("^24.6.0".to_string()),
        }),
    );
}

#[test]
fn environment_skips_a_node_pin_that_does_not_ask_for_a_download() {
    let dir = tempdir().unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "devEngines": { "runtime": { "name": "node", "version": "24.6.0", "onFail": "error" } },
        "scripts": { "prepare": "tsc" },
    });
    let environment = detect_build_environment(dir.path(), dir.path(), &manifest);
    dbg!(&environment);
    assert_eq!(environment.and_then(|environment| environment.node), None);
}

/// An addon the prepare compiles has to load in the project's Node.js.
#[test]
fn environment_keeps_a_native_build_on_the_project_runtime() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("binding.gyp"), "{}").unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "devEngines": { "runtime": { "name": "node", "version": "24.6.0", "onFail": "download" } },
        "scripts": { "prepare": "node-gyp rebuild" },
    });
    let environment = detect_build_environment(dir.path(), dir.path(), &manifest);
    dbg!(&environment);
    assert_eq!(environment.and_then(|environment| environment.node), None);
}

/// The package manager is read from the repository root, the build from
/// the package's own directory.
#[test]
fn environment_reads_the_lockfile_at_the_repository_root() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("bun.lock"), "{}").unwrap();
    let package = root.path().join("packages/x");
    fs::create_dir_all(&package).unwrap();
    let manifest = json!({ "name": "x", "version": "0.0.0", "scripts": { "prepare": "tsc" } });
    let environment = detect_build_environment(root.path(), package.as_path(), &manifest);
    dbg!(&environment);
    assert_eq!(
        environment.map(|environment| environment.package_manager.pm),
        Some(PreferredPm::Bun),
    );
}
