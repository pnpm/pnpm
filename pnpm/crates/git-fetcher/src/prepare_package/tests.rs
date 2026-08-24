use super::{
    PreparePackageOptions, PreparedPackage, package_should_be_built, prepare_package,
    safe_join_path,
};
use crate::error::PreparePackageError;
use miette::Diagnostic;
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_reporter::SilentReporter;
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{Arc, LazyLock, Mutex},
};
use tempfile::tempdir;

/// A single process-wide empty env map shared across every test
/// invocation.
fn empty_env() -> &'static HashMap<String, String> {
    static EMPTY_ENV: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);
    &EMPTY_ENV
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::write(dir.join("package.json"), serde_json::to_string(manifest).unwrap()).unwrap();
}

fn opts<'a>(allow: bool, ignore_scripts: bool) -> PreparePackageOptions<'a> {
    static EMPTY_BIN_PATHS: &[std::path::PathBuf] = &[];
    PreparePackageOptions {
        allow_build: Box::new(move |_dep_path| allow),
        pkg_resolution_id: "https://example.com/x.tgz",
        ignore_scripts,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        extra_bin_paths: EMPTY_BIN_PATHS,
        extra_env: empty_env(),
    }
}

fn opts_allow_registry_artifacts_only<'a>() -> PreparePackageOptions<'a> {
    static EMPTY_BIN_PATHS: &[std::path::PathBuf] = &[];
    PreparePackageOptions {
        allow_build: Box::new(move |dep_path| !dep_path.contains("://")),
        pkg_resolution_id: "https://example.com/x.tgz",
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        extra_bin_paths: EMPTY_BIN_PATHS,
        extra_env: empty_env(),
    }
}

fn opts_allow_dep_path<'a>(
    dep_path: &'a str,
    pkg_resolution_id: &'a str,
) -> PreparePackageOptions<'a> {
    static EMPTY_BIN_PATHS: &[std::path::PathBuf] = &[];
    PreparePackageOptions {
        allow_build: Box::new(move |actual_dep_path| actual_dep_path == dep_path),
        pkg_resolution_id,
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
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
        PreparePackageError::NotAllowed { name, version, .. } => {
            assert_eq!(name, "naughty");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected NotAllowed, got {other:?}"),
    }
}

#[test]
fn prepare_rejection_suggests_the_allow_builds_key_the_gate_checked() {
    // The bare package name cannot approve a git artifact, so an example
    // built from it sends the reader in a circle: they add the entry the
    // error asked for and the next install fails the same way.
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        &json!({
            "name": "naughty", "version": "1.0.0",
            "scripts": { "prepare": "tsc" },
        }),
    );
    let checked = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = Arc::clone(&checked);
    let mut opts = opts(false, false);
    opts.allow_build = Box::new(move |dep_path| {
        recorder.lock().unwrap().push(dep_path.to_string());
        false
    });

    let err = prepare_package::<SilentReporter>(&opts, dir.path(), None).unwrap_err();
    let help = err.help().expect("NotAllowed carries a help message").to_string();
    let checked = checked.lock().unwrap();
    let [gated_key] = checked.as_slice() else {
        panic!("expected exactly one allowBuild check, got {checked:?}");
    };
    assert!(
        help.contains(&format!("  {gated_key}: true")),
        "the help must quote the key the gate checked ({gated_key}), got: {help}",
    );
    assert!(
        !help.contains("  naughty: true"),
        "a bare-name entry never approves a git artifact, got: {help}",
    );
}

#[test]
fn prepare_rejection_keeps_resolution_id_credentials_out_of_the_diagnostic() {
    // The suggested key is built from the resolution id, which for a
    // private repository can carry the credentials git authenticated
    // with. Rendering it puts them on a terminal and into CI logs.
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        &json!({
            "name": "naughty", "version": "1.0.0",
            "scripts": { "prepare": "tsc" },
        }),
    );
    let mut opts = opts(false, false);
    opts.pkg_resolution_id =
        "git+https://s3cr3t-token:hunter2@github.com/foo/bar.git#0123456789abcdef";

    let err = prepare_package::<SilentReporter>(&opts, dir.path(), None).unwrap_err();
    let rendered = format!("{err}{}", err.help().expect("NotAllowed carries a help message"));
    for secret in ["s3cr3t-token", "hunter2"] {
        assert!(!rendered.contains(secret), "{secret:?} leaked into the diagnostic: {rendered}");
    }
    assert!(
        rendered.contains("github.com/foo/bar.git#0123456789abcdef"),
        "the repository the reader has to allow must survive redaction: {rendered}",
    );
}

#[test]
fn prepare_rejects_untrusted_manifest_identity() {
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        &json!({
            "name": "naughty", "version": "1.0.0",
            "scripts": { "prepare": "tsc" },
        }),
    );

    let err =
        prepare_package::<SilentReporter>(&opts_allow_registry_artifacts_only(), dir.path(), None)
            .unwrap_err();
    match err {
        PreparePackageError::NotAllowed { name, version, .. } => {
            assert_eq!(name, "naughty");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected NotAllowed, got {other:?}"),
    }
}

#[test]
fn prepare_allows_untrusted_manifest_identity_by_dep_path() {
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        &json!({
            "name": "trusted-name",
            "version": "1.0.0",
            "scripts": { "prepack": r#"node -e "require('fs').writeFileSync('built.txt', 'ok')""# },
        }),
    );

    // The policy sees `<manifest name>@<resolution id>` — the key a
    // lockfile would record — not the bare resolution id.
    let pkg_resolution_id = "git+https://example.com/org/repo.git#abc123";
    let dep_path = format!("trusted-name@{pkg_resolution_id}");
    let result = prepare_package::<SilentReporter>(
        &opts_allow_dep_path(&dep_path, pkg_resolution_id),
        dir.path(),
        None,
    )
    .expect("depPath-specific allow should permit prepare");

    assert!(result.should_be_built);
    assert!(dir.path().join("built.txt").exists());
}

#[test]
fn safe_join_path_rejects_escapes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let err = safe_join_path(root, Some("../escape")).unwrap_err();
    assert!(matches!(err, PreparePackageError::InvalidPath { .. }));
}

#[test]
fn safe_join_path_rejects_missing_sub_dir() {
    let dir = tempdir().unwrap();
    let err = safe_join_path(dir.path(), Some("does/not/exist")).unwrap_err();
    assert!(matches!(err, PreparePackageError::InvalidPath { .. }));
}

/// A resolution's `path` keeps the leading slash of the
/// `#path:/packages/foo` specifier it came from, and that slash is
/// rooted at the repo, not the filesystem.
#[test]
fn safe_join_path_treats_a_leading_slash_as_repo_relative() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("packages/foo")).unwrap();

    let joined = safe_join_path(root, Some("/packages/foo")).expect("repo-rooted sub-directory");
    assert_eq!(joined, root.join("packages/foo"));
}

/// Stripping the leading slash must not open a way out of the checkout.
#[test]
fn safe_join_path_rejects_an_escape_behind_a_leading_slash() {
    let dir = tempdir().unwrap();
    let err = safe_join_path(dir.path(), Some("/../escape")).unwrap_err();
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
