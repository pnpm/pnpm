use super::{
    AddMockedRegistry, CommandTempCwd, fs, fs_remove_dir_all, is_real_dir, pacquet_at,
    read_pkg_version, write_workspace_yaml,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::path::Path;

fn write_workspace_with_conflicting_ms(workspace: &Path) {
    write_workspace_yaml(
        workspace,
        "nodeLinker: hoisted\nmodulesDir: vendor\npackages:\n  - 'packages/*'\n",
    );
    let root_manifest = serde_json::json!({
        "name": "ws-root",
        "version": "0.0.0",
        "private": true,
        "dependencies": { "ms": "2.1.3" },
    });
    fs::write(workspace.join("package.json"), root_manifest.to_string())
        .expect("write root package.json");
    fs::create_dir_all(workspace.join("packages/foo")).expect("mkdir packages/foo");
    let foo_manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0",
        "dependencies": { "ms": "2.0.0" },
    });
    fs::write(workspace.join("packages/foo/package.json"), foo_manifest.to_string())
        .expect("write packages/foo/package.json");
}

fn assert_ms_in_vendor_dirs(workspace: &Path) {
    assert!(is_real_dir(workspace, "vendor/ms"));
    assert_eq!(read_pkg_version(workspace, "vendor/ms"), "2.1.3");
    assert!(is_real_dir(workspace, "packages/foo/vendor/ms"));
    assert_eq!(read_pkg_version(workspace, "packages/foo/vendor/ms"), "2.0.0");
    assert!(!workspace.join("node_modules/ms").exists());
    assert!(!workspace.join("packages/foo/node_modules/ms").exists());
}

#[test]
fn a_project_dependency_kept_in_the_project_goes_in_its_modules_dir() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_conflicting_ms(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_ms_in_vendor_dirs(&workspace);

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_ms_in_vendor_dirs(&workspace);

    fs_remove_dir_all(&workspace.join("vendor"));
    fs_remove_dir_all(&workspace.join("packages/foo/vendor"));
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_ms_in_vendor_dirs(&workspace);

    drop((root, mock_instance));
}

#[test]
fn an_unplanned_package_in_a_modules_dir_is_quarantined() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_conflicting_ms(&workspace);
    pacquet
        .with_arg("install")
        .assert()
        .success();

    for modules_dir in ["vendor", "packages/foo/vendor"] {
        let stray = workspace.join(modules_dir).join("stray");
        fs::create_dir_all(&stray).expect("mkdir stray");
        fs::write(stray.join("package.json"), r#"{"name":"stray","version":"1.0.0"}"#)
            .expect("write stray package.json");
    }
    pacquet_at(&workspace)
        .with_args(["install", "--force"])
        .assert()
        .success();

    for modules_dir in ["vendor", "packages/foo/vendor"] {
        let modules_dir = workspace.join(modules_dir);
        eprintln!("checking {modules_dir:?}");
        assert!(!modules_dir.join("stray").exists());
        assert!(modules_dir.join(".ignored/stray/package.json").exists());
    }

    drop((root, mock_instance));
}

/// Only the recorded locations name the copy in `node_modules`. A changed
/// manifest makes the install relink rather than report it up to date.
#[test]
fn an_install_removes_a_project_copy_an_earlier_layout_left_in_node_modules() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_conflicting_ms(&workspace);
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let legacy_copy = workspace.join("packages/foo/node_modules/ms");
    fs::create_dir_all(legacy_copy.parent().unwrap()).expect("mkdir packages/foo/node_modules");
    fs::rename(workspace.join("packages/foo/vendor/ms"), &legacy_copy).expect("move the copy");
    let modules_yaml = workspace.join("vendor/.modules.yaml");
    let manifest = fs::read_to_string(&modules_yaml).expect("read .modules.yaml");
    assert!(manifest.contains("packages/foo/vendor/ms"), "{manifest}");
    let legacy_manifest =
        manifest.replace("packages/foo/vendor/ms", "packages/foo/node_modules/ms");
    fs::write(&modules_yaml, legacy_manifest).expect("write .modules.yaml");

    let foo_manifest = serde_json::json!({
        "name": "foo",
        "version": "1.0.0",
        "dependencies": { "ms": "~2.0.0" },
    });
    fs::write(workspace.join("packages/foo/package.json"), foo_manifest.to_string())
        .expect("write packages/foo/package.json");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(!legacy_copy.exists());
    assert_ms_in_vendor_dirs(&workspace);

    drop((root, mock_instance));
}
