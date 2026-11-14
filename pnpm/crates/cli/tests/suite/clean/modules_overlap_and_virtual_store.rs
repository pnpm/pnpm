use super::{CommandTempCwd, seed_package};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, process::Command};

#[test]
fn clean_rootless_wildcard_prefix_preserves_members_under_overlapping_modules_dir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - '*/*'\nmodulesDir: packages\n",
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(pkg_a.join("src")).expect("create pkg-a/src");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .expect("write pkg-a manifest");
    fs::write(pkg_a.join("src/index.js"), "module.exports = 1;\n").expect("write pkg-a source");
    seed_package(&pkg_a.join("packages"), "seeded-dep");
    fs::create_dir_all(pkg_a.join("packages/broken")).expect("create broken member dep dir");
    fs::write(pkg_a.join("packages/broken/package.json"), "{ invalid json")
        .expect("write broken member dep manifest");

    let out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("clean")
        .output()
        .expect("run clean");
    assert!(out.status.success(), "{out:?}");
    assert!(!pkg_a.join("packages/seeded-dep").exists(), "member modulesDir removed");
    assert!(!pkg_a.join("packages/broken").exists(), "member broken dep removed");
    assert!(pkg_a.join("package.json").exists(), "member manifest preserved");
    assert!(pkg_a.join("src/index.js").exists(), "member source preserved");

    drop(root);
}

#[test]
fn clean_rootless_globstar_under_prefix_preserves_members_and_cleans_hoisted_deps() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/**'\nmodulesDir: packages/modules\n",
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/modules/pkg-a");
    fs::create_dir_all(pkg_a.join("src")).expect("create pkg-a/src");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .expect("write pkg-a manifest");
    fs::write(pkg_a.join("src/index.js"), "module.exports = 1;\n").expect("write pkg-a source");
    seed_package(&pkg_a.join("packages/modules"), "seeded-dep");
    fs::create_dir_all(pkg_a.join("packages/modules/broken"))
        .expect("create broken member dep dir");
    fs::write(pkg_a.join("packages/modules/broken/package.json"), "{ invalid json")
        .expect("write broken member dep manifest");

    let out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("clean")
        .output()
        .expect("run clean");
    assert!(out.status.success(), "{out:?}");
    assert!(!pkg_a.join("packages/modules/seeded-dep").exists(), "member modulesDir removed");
    assert!(!pkg_a.join("packages/modules/broken").exists(), "member broken dep removed");
    assert!(pkg_a.join("package.json").exists(), "member manifest preserved");
    assert!(pkg_a.join("src/index.js").exists(), "member source preserved");

    // When `packages/modules` holds only hoisted installed packages recorded in `.modules.yaml`,
    // `clean` still removes them rather than mistaking them for workspace members.
    fs::remove_dir_all(&pkg_a).expect("remove pkg-a inside modulesDir");
    let pkg_b = workspace.join("packages/pkg-b");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");
    fs::write(pkg_b.join("package.json"), r#"{"name":"pkg-b","version":"1.0.0"}"#)
        .expect("write pkg-b manifest");
    let root_modules = workspace.join("packages/modules");
    seed_package(&root_modules, "lodash");
    fs::write(
        root_modules.join(".modules.yaml"),
        r#"{"hoistedLocations":{"lodash@4.17.21":["packages/modules/lodash"]}}"#,
    )
    .expect("write .modules.yaml");

    let clean_hoisted = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("clean")
        .output()
        .expect("run clean on hoisted root modulesDir");
    assert!(clean_hoisted.status.success(), "{clean_hoisted:?}");
    assert!(!root_modules.join("lodash").exists(), "hoisted dependency removed");
    assert!(pkg_b.join("package.json").exists(), "pkg-b preserved");

    drop(root);
}

#[test]
fn clean_preserves_workspace_members_when_virtual_store_dir_overlaps() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - '*/*'\nvirtualStoreDir: packages\n",
    )
    .expect("write pnpm-workspace.yaml");
    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(pkg_a.join("src")).expect("create pkg-a/src");
    fs::write(pkg_a.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .expect("write pkg-a manifest");
    fs::write(pkg_a.join("src/index.js"), "module.exports = 1;\n").expect("write pkg-a source");
    seed_package(&pkg_a.join("node_modules"), "seeded-dep");

    let out = Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("clean")
        .output()
        .expect("run clean");
    assert!(out.status.success(), "{out:?}");
    assert!(!pkg_a.join("node_modules/seeded-dep").exists(), "member node_modules cleaned");
    assert!(pkg_a.join("package.json").exists(), "member manifest preserved");
    assert!(pkg_a.join("src/index.js").exists(), "member source preserved");

    drop(root);
}
