//! `pnpm import` of dependencies Yarn declares with its `patch:` protocol.

use crate::{_utils::append_workspace_yaml_key, patch::MARKER_PATCH};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const IS_POSITIVE_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch"
);

const ROOT_PATCH_PATH: &str = ".yarn/patches/is-positive-npm-1.0.0-0a1b2c3d4e.patch";

const YARN_LOCKFILE: &str = r#"__metadata:
  version: 8
  cacheKey: 10c0

"is-positive@npm:1.0.0":
  version: 1.0.0
  resolution: "is-positive@npm:1.0.0"
  languageName: node
  linkType: hard
"#;

fn write_file(dir: &Path, relative_path: &str, contents: &str) {
    let path = dir.join(relative_path);
    fs::create_dir_all(path.parent().expect("a file path has a parent"))
        .expect("create parent dir");
    fs::write(path, contents).expect("write fixture file");
}

fn read_manifest(path: &Path) -> Value {
    let text = fs::read_to_string(path).expect("read package.json");
    serde_json::from_str(&text).expect("parse package.json")
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn import_converts_yarn_patches_into_patched_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    append_workspace_yaml_key(&workspace, "packages", r#"["packages/*"]"#);
    let root_specifier = format!("patch:is-positive@npm%3A1.0.0#~/{ROOT_PATCH_PATH}");
    let root_manifest =
        json!({ "name": "root", "dependencies": { "is-positive": root_specifier } });
    write_file(&workspace, "package.json", &root_manifest.to_string());
    write_file(&workspace, ROOT_PATCH_PATH, IS_POSITIVE_PATCH);
    let foo_manifest = json!({
        "name": "foo",
        "devDependencies": {
            "@pnpm.e2e/foo": "patch:@pnpm.e2e/foo@npm%3A1.0.0#./foo.patch::locator=foo%40workspace%3Apackages%2Ffoo",
        },
    });
    write_file(&workspace, "packages/foo/package.json", &foo_manifest.to_string());
    write_file(&workspace, "packages/foo/foo.patch", MARKER_PATCH);
    write_file(&workspace, "yarn.lock", YARN_LOCKFILE);

    pacquet
        .with_arg("import")
        .assert()
        .success();

    let root_manifest = read_manifest(&workspace.join("package.json"));
    assert_eq!(root_manifest["dependencies"]["is-positive"], "1.0.0");
    let foo_manifest = read_manifest(&workspace.join("packages/foo/package.json"));
    assert_eq!(foo_manifest["devDependencies"]["@pnpm.e2e/foo"], "1.0.0");

    let workspace_yaml = read_text(&workspace.join("pnpm-workspace.yaml"));
    assert!(
        workspace_yaml.contains(&format!("is-positive@1.0.0: {ROOT_PATCH_PATH}")),
        "pnpm-workspace.yaml:\n{workspace_yaml}",
    );
    assert!(
        workspace_yaml.contains("'@pnpm.e2e/foo@1.0.0': packages/foo/foo.patch"),
        "pnpm-workspace.yaml:\n{workspace_yaml}",
    );
    let lockfile = read_text(&workspace.join("pnpm-lock.yaml"));
    assert!(lockfile.contains("version: 1.0.0(patch_hash="), "pnpm-lock.yaml:\n{lockfile}");
    assert!(!lockfile.contains("link:"), "pnpm-lock.yaml:\n{lockfile}");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let index_js = read_text(&workspace.join("node_modules/is-positive/index.js"));
    assert!(index_js.contains("// patched"), "is-positive/index.js:\n{index_js}");
    assert!(workspace.join("packages/foo/node_modules/@pnpm.e2e/foo/patched-marker.txt").exists());

    drop((root, mock_instance));
}

#[test]
fn import_warns_about_a_missing_yarn_patch_file() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let specifier = format!("patch:is-positive@npm%3A1.0.0#~/{ROOT_PATCH_PATH}");
    let manifest = json!({ "name": "root", "dependencies": { "is-positive": specifier } });
    write_file(&workspace, "package.json", &manifest.to_string());
    write_file(&workspace, "yarn.lock", YARN_LOCKFILE);

    let output = pacquet
        .with_arg("import")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains(r#""is-positive" was imported without the patch"#),
        "stdout:\n{stdout}",
    );
    let manifest = read_manifest(&workspace.join("package.json"));
    assert_eq!(manifest["dependencies"]["is-positive"], "1.0.0");
    let workspace_yaml = read_text(&workspace.join("pnpm-workspace.yaml"));
    assert!(
        !workspace_yaml.contains("patchedDependencies"),
        "pnpm-workspace.yaml:\n{workspace_yaml}",
    );
    let lockfile = read_text(&workspace.join("pnpm-lock.yaml"));
    assert!(lockfile.contains("version: 1.0.0\n"), "pnpm-lock.yaml:\n{lockfile}");

    drop((root, mock_instance));
}

#[test]
fn import_warns_about_a_dependency_with_several_yarn_patches() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let specifier =
        "patch:is-positive@npm%3A1.0.0#optional!builtin<compat/is-positive>&./a.patch&./b.patch";
    let manifest = json!({ "name": "root", "dependencies": { "is-positive": specifier } });
    write_file(&workspace, "package.json", &manifest.to_string());
    write_file(&workspace, "a.patch", IS_POSITIVE_PATCH);
    write_file(&workspace, "b.patch", MARKER_PATCH);
    write_file(&workspace, "yarn.lock", YARN_LOCKFILE);

    let output = pacquet
        .with_arg("import")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains(
            r#""is-positive" has several Yarn patches, and pnpm applies one patch per dependency."#
        ),
        "stdout:\n{stdout}",
    );
    let manifest = read_manifest(&workspace.join("package.json"));
    assert_eq!(manifest["dependencies"]["is-positive"], "1.0.0");
    let workspace_yaml = read_text(&workspace.join("pnpm-workspace.yaml"));
    assert!(
        !workspace_yaml.contains("patchedDependencies"),
        "pnpm-workspace.yaml:\n{workspace_yaml}",
    );

    drop((root, mock_instance));
}
