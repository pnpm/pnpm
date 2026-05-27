use super::{
    PreparePackageOptions, PreparedPackage, package_should_be_built, prepare_package,
    safe_join_path,
};
use crate::error::PreparePackageError;
use pacquet_executor::ScriptsPrependNodePath;
use pacquet_reporter::SilentReporter;
use serde_json::json;
use std::{collections::HashMap, fs, path::Path, sync::OnceLock};
use tempfile::tempdir;

/// A single process-wide empty env map shared across every test
/// invocation. `OnceLock` avoids the per-call `Box::leak(Box::new(...))`
/// that an earlier version of this helper used — the leak was benign
/// because the test binary exits quickly, but accumulating one fresh
/// allocation per test isn't necessary when every site wants the same
/// value.
fn empty_env() -> &'static HashMap<String, String> {
    static M: OnceLock<HashMap<String, String>> = OnceLock::new();
    M.get_or_init(HashMap::new)
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::write(dir.join("package.json"), serde_json::to_string(manifest).unwrap()).unwrap();
}

/// Build an `Options` value whose `allow_build` closure routes through
/// the bool the test specifies. Other knobs default to "noop, no
/// scripts run" so the test doesn't actually spawn anything unless we
/// want it to.
fn opts<'a>(allow: bool, ignore_scripts: bool) -> PreparePackageOptions<'a> {
    static EMPTY_BIN_PATHS: &[std::path::PathBuf] = &[];
    PreparePackageOptions {
        allow_build: Box::new(move |_name, _version| allow),
        ignore_scripts,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        extra_bin_paths: EMPTY_BIN_PATHS,
        extra_env: empty_env(),
    }
}

#[test]
fn package_should_be_built_false_for_empty_scripts() {
    let dir = tempdir().unwrap();
    let manifest = json!({ "name": "x", "version": "0.0.0" });
    assert!(!package_should_be_built(&manifest, dir.path()));
}

#[test]
fn package_should_be_built_true_for_non_empty_prepare() {
    let dir = tempdir().unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "scripts": { "prepare": "tsc" },
    });
    assert!(package_should_be_built(&manifest, dir.path()));
}

#[test]
fn package_should_be_built_false_when_main_exists_and_prepare_absent() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.js"), "").unwrap();
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "scripts": { "prepublish": "true" },
    });
    // Prepublish is set, main exists → upstream says "don't build".
    assert!(!package_should_be_built(&manifest, dir.path()));
}

#[test]
fn package_should_be_built_true_when_main_missing_and_prepublish_set() {
    let dir = tempdir().unwrap();
    // `main` defaults to `index.js`; create no file so it's missing.
    let manifest = json!({
        "name": "x", "version": "0.0.0",
        "scripts": { "prepack": "rollup -c" },
    });
    assert!(package_should_be_built(&manifest, dir.path()));
}

#[test]
fn prepare_returns_should_be_built_false_when_no_manifest() {
    let dir = tempdir().unwrap();
    let received =
        prepare_package::<SilentReporter>(&opts(false, false), dir.path(), None).unwrap();
    assert!(!received.should_be_built);
    assert_eq!(received.pkg_dir, dir.path());
}

#[test]
fn prepare_returns_should_be_built_false_when_manifest_has_no_scripts() {
    let dir = tempdir().unwrap();
    write_manifest(dir.path(), &json!({ "name": "x", "version": "0.0.0" }));

    let PreparedPackage { pkg_dir, should_be_built } =
        prepare_package::<SilentReporter>(&opts(false, false), dir.path(), None).unwrap();
    assert!(!should_be_built);
    assert_eq!(pkg_dir, dir.path());
}

#[test]
fn prepare_ignore_scripts_short_circuits_without_spawn() {
    // The script body would fail if it actually ran, so observing
    // success proves we short-circuited before spawning.
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        &json!({
            "name": "x", "version": "0.0.0",
            "scripts": { "prepare": "exit 1" },
        }),
    );

    let PreparedPackage { should_be_built, .. } =
        prepare_package::<SilentReporter>(&opts(true, true), dir.path(), None).unwrap();
    assert!(should_be_built, "ignore_scripts still reports should_be_built");
}

#[test]
fn prepare_rejects_when_allow_build_returns_false() {
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        &json!({
            "name": "naughty", "version": "1.0.0",
            "scripts": { "prepare": "tsc" },
        }),
    );

    let err = prepare_package::<SilentReporter>(&opts(false, false), dir.path(), None).unwrap_err();
    match err {
        PreparePackageError::NotAllowed { name, version } => {
            assert_eq!(name, "naughty");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected NotAllowed, got {other:?}"),
    }
}

#[test]
fn safe_join_path_rejects_escapes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // `..` escape — canonical form lives outside `root`.
    let err = safe_join_path(root, Some("../escape")).unwrap_err();
    assert!(matches!(err, PreparePackageError::InvalidPath { .. }));
}

#[test]
fn safe_join_path_rejects_missing_sub_dir() {
    let dir = tempdir().unwrap();
    let err = safe_join_path(dir.path(), Some("does/not/exist")).unwrap_err();
    assert!(matches!(err, PreparePackageError::InvalidPath { .. }));
}

#[test]
fn safe_join_path_accepts_empty_sub_dir() {
    let dir = tempdir().unwrap();
    let received = safe_join_path(dir.path(), None).unwrap();
    let canonical_root = dir.path().canonicalize().unwrap();
    let canonical_received = received.canonicalize().unwrap();
    assert_eq!(canonical_received, canonical_root);
}
