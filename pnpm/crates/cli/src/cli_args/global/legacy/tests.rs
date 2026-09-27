use super::{LegacyGlobalLayout, MigrationSelector, is_legacy_bin, legacy_bin_files};
use pnpm_fs::lexical_normalize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn write_legacy_manifest(global_dir: &Path, dependencies: &Value) -> PathBuf {
    let legacy_dir = global_dir.join("5");
    fs::create_dir_all(&legacy_dir).expect("create the legacy global dir");
    fs::write(legacy_dir.join("package.json"), json!({ "dependencies": dependencies }).to_string())
        .expect("write the legacy global manifest");
    legacy_dir
}

fn selector(alias: &str, selector: &str) -> MigrationSelector {
    MigrationSelector { alias: alias.to_string(), selector: selector.to_string() }
}

fn sorted(mut selectors: Vec<MigrationSelector>) -> Vec<MigrationSelector> {
    selectors.sort_by(|a, b| a.alias.cmp(&b.alias));
    selectors
}

#[test]
fn find_returns_none_without_a_legacy_manifest() {
    let root = TempDir::new().expect("create temp dir");
    let global_dir = root.path().join("global");
    fs::create_dir_all(global_dir.join("5")).expect("create an empty legacy dir");

    let legacy = LegacyGlobalLayout::find(&global_dir.join("v11")).expect("read the legacy dir");

    assert!(legacy.is_none());
}

#[test]
fn find_rejects_a_legacy_manifest_that_does_not_parse() {
    let root = TempDir::new().expect("create temp dir");
    let global_dir = root.path().join("global");
    let legacy_dir = global_dir.join("5");
    fs::create_dir_all(&legacy_dir).expect("create the legacy global dir");
    fs::write(legacy_dir.join("package.json"), "{").expect("write a broken manifest");

    let error = LegacyGlobalLayout::find(&global_dir.join("v11"))
        .err()
        .expect("a manifest that does not parse must not read as empty");

    let message = error.to_string();
    assert!(message.contains("package.json"), "{message}");
}

#[test]
fn selectors_skip_pnpm_and_the_packages_a_current_group_installs() {
    let root = TempDir::new().expect("create temp dir");
    let global_dir = root.path().join("global");
    write_legacy_manifest(
        &global_dir,
        &json!({
            "typescript": "^5.4.0",
            "pnpm": "^10.0.0",
            "@pnpm/exe": "10.0.0",
            "pm": "npm:pnpm@10",
            "eslint": "9.0.0",
        }),
    );
    let legacy = LegacyGlobalLayout::find(&global_dir.join("v11"))
        .expect("read the legacy manifest")
        .expect("find the legacy layout");
    let installed = BTreeSet::from(["eslint".to_string()]);

    let selectors = legacy.selectors_to_migrate(&installed);

    assert_eq!(selectors, [selector("typescript", "typescript@^5.4.0")]);
}

#[test]
fn selectors_anchor_relative_local_paths_at_the_legacy_project() {
    let root = TempDir::new().expect("create temp dir");
    let global_dir = root.path().join("global");
    let legacy_dir = write_legacy_manifest(
        &global_dir,
        &json!({
            "my-tool": "link:../../tools/my-tool",
            "other-tool": "file:/tools/other-tool",
        }),
    );
    let legacy = LegacyGlobalLayout::find(&global_dir.join("v11"))
        .expect("read the legacy manifest")
        .expect("find the legacy layout");

    let selectors = sorted(legacy.selectors_to_migrate(&BTreeSet::new()));

    let my_tool = lexical_normalize(&legacy_dir.join("../../tools/my-tool"));
    assert_eq!(
        selectors,
        [
            selector("my-tool", &format!("my-tool@link:{}", my_tool.display())),
            selector("other-tool", "other-tool@file:/tools/other-tool"),
        ],
    );
}

fn home_with_legacy_dir() -> (TempDir, PathBuf, PathBuf) {
    let root = TempDir::new().expect("create temp dir");
    let home = root.path().join("pnpm-home");
    let legacy_dir = home.join("global").join("5");
    fs::create_dir_all(legacy_dir.join("node_modules")).expect("create the legacy dir");
    (root, home, legacy_dir)
}

#[test]
fn is_legacy_bin_accepts_a_shim_that_runs_the_legacy_project_relative_to_itself() {
    let (_root, home, legacy_dir) = home_with_legacy_dir();
    let shim = home.join("tsc");
    fs::write(
        &shim,
        "#!/bin/sh\nexec node \"$basedir/global/5/node_modules/typescript/bin/tsc\" \"$@\"\n",
    )
    .expect("write the sh shim");

    assert!(is_legacy_bin(&shim, &legacy_dir));
}

#[test]
fn is_legacy_bin_accepts_a_cmd_shim_with_backslashes_and_an_absolute_target() {
    let (_root, home, legacy_dir) = home_with_legacy_dir();
    let shim = home.join("tsc.cmd");
    let target = legacy_dir
        .join("node_modules")
        .join("typescript")
        .join("bin")
        .join("tsc");
    let target = target
        .display()
        .to_string()
        .replace('/', r"\");
    let content = format!("@node \"{target}\" %*\r\n");
    fs::write(&shim, content).expect("write the cmd shim");

    assert!(is_legacy_bin(&shim, &legacy_dir));
}

#[cfg(unix)]
#[test]
fn is_legacy_bin_accepts_a_symlink_into_the_legacy_project() {
    let (_root, home, legacy_dir) = home_with_legacy_dir();
    let link = home.join("tsc");
    std::os::unix::fs::symlink("global/5/node_modules/typescript/bin/tsc", &link)
        .expect("link into the legacy project");

    assert!(is_legacy_bin(&link, &legacy_dir));
}

#[test]
fn is_legacy_bin_keeps_a_shim_of_the_current_layout_and_an_unrelated_file() {
    let (_root, home, legacy_dir) = home_with_legacy_dir();
    let current = home.join("tsc");
    fs::write(
        &current,
        "#!/bin/sh\nexec node \"$basedir/global/v11/abc/node_modules/typescript/bin/tsc\" \"$@\"\n",
    )
    .expect("write the current shim");
    let unrelated = home.join("my-script");
    fs::write(&unrelated, "#!/bin/sh\necho hi\n").expect("write an unrelated script");

    assert!(!is_legacy_bin(&current, &legacy_dir));
    assert!(!is_legacy_bin(&unrelated, &legacy_dir));
    assert!(!is_legacy_bin(&home.join("missing"), &legacy_dir));
}

#[test]
fn legacy_bin_files_lists_only_the_files_that_point_into_the_legacy_project() {
    let (_root, home, legacy_dir) = home_with_legacy_dir();
    fs::write(
        home.join("tool.cmd"),
        "@node \"%~dp0\\global\\5\\node_modules\\tool\\cli.js\" %*\r\n",
    )
    .expect("write the legacy cmd shim");
    fs::write(home.join("tool.exe"), "MZ").expect("write an unrelated executable");
    fs::write(home.join("tool"), "#!/bin/sh\necho mine\n").expect("write an unrelated script");

    let files = legacy_bin_files(&home.join("tool"), &legacy_dir);

    assert_eq!(files, [home.join("tool.cmd")]);
}
