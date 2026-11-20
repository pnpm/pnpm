use crate::_utils::pacquet_in;

use assert_cmd::prelude::*;
use std::{
    fs,
    path::Path,
    process::Command,
};
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

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::create_dir_all(path.parent().expect("parent directory")).expect("create parent directory");
    fs::write(path, value.to_string()).expect("write JSON file");
}

fn assert_frozen_install_is_outdated(workspace: &Path) {
    let result = pnpm_at(workspace)
        .args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&result.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"), "stderr: {stderr}");
}

/// The parent is a directory outside the workspace, so no importer entry
/// records its children and only the snapshot check can see the change.
#[test]
fn frozen_install_rejects_a_changed_injected_setting_of_a_local_directory_child() {
    let (_root, workspace) = workspace();
    write_json(&workspace.join("package.json"), &serde_json::json!({ "name": "root" }));
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace configuration");
    write_json(
        &workspace.join("packages/app/package.json"),
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "file:../../vendor/lib" },
        }),
    );
    write_json(
        &workspace.join("packages/workspace-child/package.json"),
        &serde_json::json!({ "name": "workspace-child", "version": "1.0.0" }),
    );
    write_json(
        &workspace.join("vendor/path-child/package.json"),
        &serde_json::json!({ "name": "path-child", "version": "1.0.0" }),
    );
    let write_lib = |injected: &[&str]| {
        let mut manifest = serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "dependencies": { "path-child": "../path-child", "workspace-child": "workspace:*" },
        });
        for child in injected {
            manifest["dependenciesMeta"][child]["injected"] = serde_json::json!(true);
        }
        write_json(&workspace.join("vendor/lib/package.json"), &manifest);
    };
    let install = |args: &[&str]| {
        pnpm_at(&workspace)
            .arg("install")
            .args(args)
            .assert()
            .success();
    };

    write_lib(&[]);
    install(&[]);
    install(&["--frozen-lockfile"]);
    for injected in [&["path-child"][..], &["workspace-child"]] {
        write_lib(injected);
        assert_frozen_install_is_outdated(&workspace);
    }

    write_lib(&["path-child", "workspace-child"]);
    install(&[]);
    install(&["--frozen-lockfile"]);
    for injected in [&["path-child"][..], &["workspace-child"]] {
        write_lib(injected);
        assert_frozen_install_is_outdated(&workspace);
    }
}

#[test]
fn a_local_tarball_child_link_is_resolved_from_the_lockfile_directory() {
    let (_root, workspace) = workspace();
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create app directory");
    let child = workspace.join("child");
    let decoy = app.join("child");
    fs::create_dir_all(&child).expect("create child directory");
    fs::create_dir_all(&decoy).expect("create importer-relative decoy directory");
    fs::write(child.join("package.json"), r#"{"name":"child","version":"1.0.0"}"#)
        .expect("write child package.json");
    fs::write(decoy.join("package.json"), r#"{"name":"child","version":"2.0.0"}"#)
        .expect("write importer-relative decoy package.json");
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

    let installed_tarball =
        fs::canonicalize(app.join("node_modules/tpkg")).expect("resolve installed tarball");
    let siblings = installed_tarball.parent().expect("directory holding the tarball's children");
    for alias in ["child", "child-bare"] {
        assert_eq!(
            fs::canonicalize(siblings.join(alias)).expect("resolve installed child"),
            fs::canonicalize(&child).expect("resolve child directory"),
            "{alias}",
        );
    }
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

#[test]
fn injected_children_with_the_same_relative_path_stay_apart_across_importers() {
    let (_root, workspace) = workspace();
    write_json(&workspace.join("package.json"), &serde_json::json!({ "name": "root" }));
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - '*/app'\n")
        .expect("write workspace configuration");
    for group in ["first", "second"] {
        let group_dir = workspace.join(group);
        write_json(
            &group_dir.join("app/package.json"),
            &serde_json::json!({
                "name": format!("{group}-app"),
                "version": "1.0.0",
                "dependencies": { "lib": "file:../lib" },
            }),
        );
        write_json(
            &group_dir.join("lib/package.json"),
            &serde_json::json!({
                "name": format!("{group}-lib"),
                "version": "1.0.0",
                "dependencies": { "child": "../child" },
                "dependenciesMeta": { "child": { "injected": true } },
            }),
        );
        write_json(
            &group_dir.join("child/package.json"),
            &serde_json::json!({ "name": "child", "version": "1.0.0" }),
        );
    }

    pnpm_at(&workspace)
        .arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("child: file:first/child"), "lockfile:\n{lockfile}");
    assert!(lockfile.contains("child: file:second/child"), "lockfile:\n{lockfile}");

    pnpm_at(&workspace)
        .args(["install", "--frozen-lockfile"])
        .assert()
        .success();
}
