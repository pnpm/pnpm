use super::{
    AddMockedRegistry,
    CommandTempCwd,
    fs,
    fs_remove_dir_all,
    pacquet_at,
    read_pkg_version,
    write_workspace_yaml,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::path::Path;

/// `enclosing` keeps `ms@2.0.0`, because the other projects put `ms@2.1.3`
/// at the root. `enclosing/nested` depends on `ms@2.1.3`.
fn write_workspace_with_a_nested_project(workspace: &Path) {
    write_workspace_yaml(
        workspace,
        "nodeLinker: hoisted\npackages:\n  - enclosing\n  - enclosing/nested\n  - 'other-*'\n",
    );
    let root_manifest =
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true });
    fs::write(workspace.join("package.json"), root_manifest.to_string())
        .expect("write root package.json");
    let projects = [
        ("enclosing", "2.0.0"),
        ("enclosing/nested", "2.1.3"),
        ("other-1", "2.1.3"),
        ("other-2", "2.1.3"),
    ];
    for (dir, ms_version) in projects {
        fs::create_dir_all(workspace.join(dir)).expect("create the project directory");
        let manifest = serde_json::json!({
            "name": dir.replace('/', "-"),
            "version": "1.0.0",
            "dependencies": { "ms": ms_version },
        });
        fs::write(workspace.join(dir).join("package.json"), manifest.to_string())
            .expect("write the project manifest");
    }
}

/// From `enclosing/nested`, Node.js finds `enclosing/node_modules` before
/// the root's, so the nested project needs a copy of its own.
fn assert_each_project_resolves_its_ms(workspace: &Path) {
    assert_eq!(read_pkg_version(workspace, "node_modules/ms"), "2.1.3");
    assert_eq!(read_pkg_version(workspace, "enclosing/node_modules/ms"), "2.0.0");
    assert_eq!(read_pkg_version(workspace, "enclosing/nested/node_modules/ms"), "2.1.3");
    assert!(!workspace.join("other-1/node_modules/ms").exists());
}

#[test]
fn a_nested_project_keeps_the_dependency_the_enclosing_project_shadows() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_a_nested_project(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_each_project_resolves_its_ms(&workspace);

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_each_project_resolves_its_ms(&workspace);

    for modules_dir in ["node_modules", "enclosing/node_modules", "enclosing/nested/node_modules"] {
        fs_remove_dir_all(&workspace.join(modules_dir));
    }
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_each_project_resolves_its_ms(&workspace);

    drop((root, mock_instance));
}
