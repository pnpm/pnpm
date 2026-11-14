use crate::_utils::pacquet_in;

use assert_cmd::prelude::*;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn pnpm_at(workspace: &Path) -> Command {
    pacquet_in(workspace)
}

fn workspace() -> (TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("create temporary root");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    (root, workspace)
}

#[test]
fn frozen_install_rejects_a_changed_injected_setting_for_a_child_of_an_injected_package() {
    let (_root, workspace) = workspace();
    fs::write(workspace.join("package.json"), r#"{"name":"root"}"#)
        .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace configuration");
    for package in ["app", "lib", "child"] {
        let dir = workspace.join("packages").join(package);
        fs::create_dir_all(&dir).expect("create workspace package");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({ "name": package, "version": "1.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
    fs::write(
        workspace.join("packages/app/package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"lib":"workspace:*"},"dependenciesMeta":{"lib":{"injected":true}}}"#,
    )
    .expect("write app package.json");
    fs::write(
        workspace.join("packages/lib/package.json"),
        r#"{"name":"lib","version":"1.0.0","dependencies":{"child":"workspace:*"},"dependenciesMeta":{"child":{"injected":true}}}"#,
    )
    .expect("write lib package.json");

    pnpm_at(&workspace)
        .arg("install")
        .assert()
        .success();
    fs::write(
        workspace.join("packages/lib/package.json"),
        r#"{"name":"lib","version":"1.0.0","dependencies":{"child":"workspace:*"}}"#,
    )
    .expect("remove child injection metadata");

    let result = pnpm_at(&workspace)
        .args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&result.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"), "stderr: {stderr}");
}

#[test]
fn a_local_tarball_child_link_is_resolved_from_the_lockfile_directory() {
    let (_root, workspace) = workspace();
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create app directory");
    let child = workspace.join("child");
    fs::create_dir_all(&child).expect("create child directory");
    fs::write(child.join("package.json"), r#"{"name":"child","version":"1.0.0"}"#)
        .expect("write child package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace configuration");
    fs::write(workspace.join("package.json"), r#"{"name":"root","version":"1.0.0"}"#)
        .expect("write root package.json");
    fs::write(
        workspace.join("tpkg.tgz"),
        pnpm_testing_utils::fixtures::tarball_with_manifest(&serde_json::json!({
            "name": "tpkg",
            "version": "1.0.0",
            "dependencies": { "child": "link:child", "child-bare": "./child" },
        })),
    )
    .expect("write local tarball");
    fs::write(
        app.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"tpkg":"file:../../tpkg.tgz"}}"#,
    )
    .expect("write app package.json");

    pnpm_at(&app)
        .arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("child: link:child"), "lockfile:\n{lockfile}");
    assert!(lockfile.contains("child-bare: link:child"), "lockfile:\n{lockfile}");
}

#[test]
fn a_root_local_tarball_child_keeps_its_path_resolver_shape() {
    let (_root, workspace) = workspace();
    let child = workspace.join("child");
    fs::create_dir_all(&child).expect("create child directory");
    fs::write(child.join("package.json"), r#"{"name":"child","version":"1.0.0"}"#)
        .expect("write child package.json");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"root","dependencies":{"tpkg":"file:tpkg.tgz"}}"#,
    )
    .expect("write root package.json");
    fs::write(
        workspace.join("tpkg.tgz"),
        pnpm_testing_utils::fixtures::tarball_with_manifest(&serde_json::json!({
            "name": "tpkg",
            "version": "1.0.0",
            "dependencies": { "child": "./child" },
        })),
    )
    .expect("write local tarball");

    pnpm_at(&workspace)
        .arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("child: link:child"), "lockfile:\n{lockfile}");
}

#[test]
fn local_directory_children_are_scoped_to_their_parent_directory() {
    let (_root, workspace) = workspace();
    fs::write(workspace.join("package.json"), r#"{"name":"root","dependencies":{"first":"file:packages/first/parent","second":"file:packages/second/parent"}}"#)
        .expect("write root package.json");
    for (parent, child_name) in [("first", "first-child"), ("second", "second-child")] {
        let parent_dir = workspace
            .join("packages")
            .join(parent)
            .join("parent");
        let child = workspace
            .join("packages")
            .join(parent)
            .join("child");
        fs::create_dir_all(&parent_dir).expect("create parent directory");
        fs::create_dir_all(&child).expect("create child directory");
        let mut manifest = serde_json::json!({ "name": parent, "version": "1.0.0" });
        manifest["dependencies"][child_name] = serde_json::json!("../child");
        fs::write(parent_dir.join("package.json"), manifest.to_string())
            .expect("write parent package.json");
        fs::write(
            child.join("package.json"),
            serde_json::json!({ "name": child_name, "version": "1.0.0" }).to_string(),
        )
        .expect("write child package.json");
    }

    pnpm_at(&workspace)
        .arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("first-child: link:packages/first/child"), "lockfile:\n{lockfile}");
    assert!(lockfile.contains("second-child: link:packages/second/child"), "lockfile:\n{lockfile}");

    pnpm_at(&workspace)
        .args(["install", "--frozen-lockfile"])
        .assert()
        .success();
}
