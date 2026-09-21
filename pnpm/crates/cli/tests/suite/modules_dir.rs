//! A custom `modulesDir`: Node looks for packages in `node_modules`
//! directories only, so it cannot find the ones installed into
//! `<project>/vendor`. The command shims put that directory on `NODE_PATH`,
//! so tools the project installs can load its plugins. Mirrors
//! `pnpm11/installing/deps-installer/test/install/modulesDir.ts`.

use crate::_utils::{append_workspace_yaml_key, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

#[test]
fn bins_in_a_custom_modules_dir_load_plugins_installed_in_it() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    write_tool(&workspace);
    write_plugin(&workspace);
    write_manifest(
        &workspace,
        &serde_json::json!({
            "scripts": { "lint": "tool" },
            "dependencies": { "is-positive": "3.1.0", "plugin": "file:plugin", "tool": "file:tool" },
        }),
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        tool_stdout(&workspace),
        r#"{"plugin":"plugin loaded","isPositive":"3.1.0","ownIsPositive":"1.0.0"}"#,
    );
    for args in [["run", "lint"], ["exec", "tool"]] {
        let output = pacquet_in(&workspace)
            .with_args(args)
            .output()
            .expect("run pnpm");
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(r#""plugin":"plugin loaded""#),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stdout),
        );
    }

    fs::remove_dir_all(workspace.join("vendor")).expect("remove vendor");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(
        tool_stdout(&workspace),
        r#"{"plugin":"plugin loaded","isPositive":"3.1.0","ownIsPositive":"1.0.0"}"#,
    );

    drop((root, mock_instance));
}

#[test]
fn bins_in_a_custom_modules_dir_are_relinked_when_extend_node_path_changes() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    append_workspace_yaml_key(&workspace, "extendNodePath", "false");
    write_tool(&workspace);
    write_plugin(&workspace);
    write_manifest(
        &workspace,
        &serde_json::json!({ "dependencies": { "plugin": "file:plugin", "tool": "file:tool" } }),
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_plugin_not_found(&run_tool(&workspace));

    set_extend_node_path(&workspace, "false", "true");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        tool_stdout(&workspace),
        r#"{"plugin":"plugin loaded","isPositive":"1.0.0","ownIsPositive":"1.0.0"}"#,
    );

    set_extend_node_path(&workspace, "true", "false");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_plugin_not_found(&run_tool(&workspace));

    drop((root, mock_instance));
}

#[test]
fn bins_of_every_project_load_plugins_only_from_that_projects_modules_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    append_workspace_yaml_key(&workspace, "packages", "['project-1', 'project-2']");
    // The private hoist exposes every non-root project's dependencies to
    // all of them, whatever the modules directory.
    append_workspace_yaml_key(&workspace, "hoistPattern", "[]");
    write_tool(&workspace);
    write_plugin(&workspace);
    write_manifest(&workspace, &serde_json::json!({ "name": "root" }));
    write_manifest(
        &workspace.join("project-1"),
        &serde_json::json!({ "name": "project-1", "dependencies": { "tool": "file:../tool" } }),
    );
    write_manifest(
        &workspace.join("project-2"),
        &serde_json::json!({
            "name": "project-2",
            "dependencies": { "plugin": "file:../plugin", "tool": "file:../tool" },
        }),
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_plugin_not_found(&run_tool(&workspace.join("project-1")));
    assert_eq!(
        tool_stdout(&workspace.join("project-2")),
        r#"{"plugin":"plugin loaded","isPositive":"1.0.0","ownIsPositive":"1.0.0"}"#,
    );

    drop((root, mock_instance));
}

/// Loads plugins from the working directory, the way `ESLint` and similar
/// tools do.
fn write_tool(workspace: &Path) {
    write_manifest(
        &workspace.join("tool"),
        &serde_json::json!({
            "name": "tool",
            "version": "1.0.0",
            "bin": "bin.js",
            "dependencies": { "is-positive": "1.0.0" },
        }),
    );
    fs::write(
        workspace.join("tool/bin.js"),
        "#!/usr/bin/env node\n\
         const { createRequire } = require('node:module')\n\
         const path = require('node:path')\n\
         const requireFromProject = createRequire(path.join(process.cwd(), 'package.json'))\n\
         console.log(JSON.stringify({\n  \
           plugin: requireFromProject('plugin'),\n  \
           isPositive: requireFromProject('is-positive/package.json').version,\n\
           ownIsPositive: require('is-positive/package.json').version,\n\
         }))\n",
    )
    .expect("write tool/bin.js");
}

fn write_plugin(workspace: &Path) {
    write_manifest(
        &workspace.join("plugin"),
        &serde_json::json!({ "name": "plugin", "version": "1.0.0" }),
    );
    fs::write(workspace.join("plugin/index.js"), "module.exports = 'plugin loaded'\n")
        .expect("write plugin/index.js");
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::create_dir_all(dir).expect("create package dir");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn set_extend_node_path(workspace: &Path, from: &str, to: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let from = format!("extendNodePath: {from}\n");
    assert!(yaml.contains(&from), "{yaml}");
    fs::write(&yaml_path, yaml.replace(&from, &format!("extendNodePath: {to}\n")))
        .expect("write pnpm-workspace.yaml");
}

fn run_tool(project_dir: &Path) -> Output {
    let shim = if cfg!(windows) { "vendor/.bin/tool.cmd" } else { "vendor/.bin/tool" };
    Command::new(project_dir.join(shim))
        .current_dir(project_dir)
        .env_remove("NODE_PATH")
        .output()
        .expect("run the tool shim")
}

fn tool_stdout(project_dir: &Path) -> String {
    let output = run_tool(project_dir);
    assert!(output.status.success(), "tool failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout)
        .expect("utf-8 stdout")
        .trim_end()
        .to_string()
}

fn assert_plugin_not_found(output: &Output) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cannot find module 'plugin'"), "{stderr}");
}
