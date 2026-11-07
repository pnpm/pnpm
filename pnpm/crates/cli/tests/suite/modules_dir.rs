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
    path::{Path, PathBuf},
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

#[test]
fn project_lifecycle_scripts_run_bins_from_its_custom_modules_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    // Every script gets the root project's `.bin` through `extraBinPaths`,
    // so only a non-root project shows which `.bin` its own scripts get.
    append_workspace_yaml_key(&workspace, "packages", "['project']");
    write_tool(&workspace);
    write_plugin(&workspace);
    write_manifest(&workspace, &serde_json::json!({ "name": "root" }));
    write_manifest(
        &workspace.join("project"),
        &serde_json::json!({
            "name": "project",
            "scripts": { "postinstall": "tool > tool-output.json" },
            "dependencies": { "plugin": "file:../plugin", "tool": "file:../tool" },
        }),
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let output = fs::read_to_string(workspace.join("project/tool-output.json"))
        .expect("read tool-output.json");
    assert!(output.contains(r#""plugin":"plugin loaded""#), "{output}");

    drop((root, mock_instance));
}

/// A symlinked executable has no shim to carry `NODE_PATH`.
#[cfg_attr(windows, ignore = "executables are symlinked only on Unix")]
#[test]
fn symlinked_bins_of_every_project_get_that_projects_modules_dir_on_node_path() {
    assert_symlinked_bins_load_plugins_per_project(&[], "vendor");
}

#[cfg_attr(windows, ignore = "executables are symlinked only on Unix")]
#[test]
fn symlinked_bins_get_the_modules_dir_a_package_configs_entry_gives_the_project() {
    assert_symlinked_bins_load_plugins_per_project(
        &[
            ("sharedWorkspaceLockfile", "false"),
            ("packageConfigs", "{ project-2: { modulesDir: custom } }"),
        ],
        "custom",
    );
}

fn assert_symlinked_bins_load_plugins_per_project(
    settings: &[(&str, &str)],
    project_2_modules_dir: &str,
) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    append_workspace_yaml_key(&workspace, "packages", "['project-1', 'project-2']");
    append_workspace_yaml_key(&workspace, "preferSymlinkedExecutables", "true");
    // The private hoist would expose project-2's plugin to project-1.
    append_workspace_yaml_key(&workspace, "hoistPattern", "[]");
    for (key, value) in settings {
        append_workspace_yaml_key(&workspace, key, value);
    }
    write_probe_tool(&workspace);
    write_plugin(&workspace);
    write_manifest(&workspace, &serde_json::json!({ "name": "root" }));
    for (project, dependencies) in [
        ("project-1", serde_json::json!({ "tool": "file:../tool" })),
        ("project-2", serde_json::json!({ "plugin": "file:../plugin", "tool": "file:../tool" })),
    ] {
        write_manifest(
            &workspace.join(project),
            &serde_json::json!({
                "name": project,
                "version": "1.0.0",
                "scripts": {
                    "lint": "tool",
                    "postinstall": "tool > tool-output.txt",
                    "version": "tool > version-output.txt",
                },
                "dependencies": dependencies,
            }),
        );
    }

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let bin = workspace
        .join("project-2")
        .join(project_2_modules_dir)
        .join(".bin/tool");
    assert!(fs::symlink_metadata(&bin).expect("stat the bin").is_symlink());
    for (project, expected) in
        [("project-1", "project-1: missing"), ("project-2", "project-2: plugin loaded")]
    {
        let output = fs::read_to_string(workspace.join(project).join("tool-output.txt"))
            .expect("read tool-output.txt");
        assert_eq!(output.trim_end(), expected);
    }
    for args in [["-r", "run", "lint"], ["-r", "exec", "tool"]] {
        let stdout = probe_stdout(&workspace, &args);
        assert!(stdout.contains("project-1: missing"), "{args:?}: {stdout}");
        assert!(stdout.contains("project-2: plugin loaded"), "{args:?}: {stdout}");
    }
    for args in [&["run", "lint"][..], &["exec", "tool"]] {
        let stdout = probe_stdout(&workspace.join("project-2"), args);
        assert!(stdout.contains("project-2: plugin loaded"), "{args:?}: {stdout}");
    }
    let direct = Command::new(&bin)
        .current_dir(workspace.join("project-2"))
        .env_remove("NODE_PATH")
        .output()
        .expect("run the bin");
    assert_eq!(String::from_utf8_lossy(&direct.stdout).trim_end(), "project-2: missing");
    pacquet_in(&workspace.join("project-2"))
        .with_args(["version", "patch", "--no-git-checks"])
        .assert()
        .success();
    let output = fs::read_to_string(workspace.join("project-2/version-output.txt"))
        .expect("read version-output.txt");
    assert_eq!(output.trim_end(), "project-2: plugin loaded");

    drop((root, mock_instance));
}

#[cfg_attr(windows, ignore = "executables are symlinked only on Unix")]
#[test]
fn symlinked_bins_of_the_hoisted_linker_load_plugins_from_the_custom_modules_dir() {
    let [lint, node_path] = run_probe_in_single_project(
        &[("nodeLinker", "hoisted")],
        [&["run", "lint"], &["exec", "node", "-p", "process.env.NODE_PATH"]],
    );
    assert!(lint.contains("root: plugin loaded"), "{lint}");
    let first = node_path
        .trim_end()
        .split(':')
        .next()
        .unwrap_or_default();
    assert!(Path::new(first).ends_with("vendor"), "{node_path}");
}

#[cfg_attr(windows, ignore = "executables are symlinked only on Unix")]
#[test]
fn symlinked_bins_get_no_custom_modules_dir_on_node_path_when_extend_node_path_is_false() {
    let [lint] = run_probe_in_single_project(
        &[("preferSymlinkedExecutables", "true"), ("extendNodePath", "false")],
        [&["run", "lint"]],
    );
    assert!(lint.contains("root: missing"), "{lint}");
}

fn run_probe_in_single_project<const COMMANDS: usize>(
    settings: &[(&str, &str)],
    commands: [&[&str]; COMMANDS],
) -> [String; COMMANDS] {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    for (key, value) in settings {
        append_workspace_yaml_key(&workspace, key, value);
    }
    write_probe_tool(&workspace);
    write_plugin(&workspace);
    write_manifest(
        &workspace,
        &serde_json::json!({
            "name": "root",
            "scripts": { "lint": "tool" },
            "dependencies": { "plugin": "file:plugin", "tool": "file:tool" },
        }),
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        fs::symlink_metadata(workspace.join("vendor/.bin/tool"))
            .expect("stat the bin")
            .is_symlink(),
    );
    let stdouts = commands.map(|args| probe_stdout(&workspace, args));
    drop((root, mock_instance));
    stdouts
}

fn probe_stdout(dir: &Path, args: &[&str]) -> String {
    let output = pacquet_in(dir)
        .with_args(args)
        .output()
        .expect("run pnpm");
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write_probe_tool(workspace: &Path) {
    write_manifest(
        &workspace.join("tool"),
        &serde_json::json!({ "name": "tool", "version": "1.0.0", "bin": "bin.js" }),
    );
    fs::write(
        workspace.join("tool/bin.js"),
        "#!/usr/bin/env node\n\
         const path = require('node:path')\n\
         const requireFromProject = require('node:module').createRequire(path.join(process.cwd(), 'package.json'))\n\
         let plugin\n\
         try { plugin = requireFromProject('plugin') } catch { plugin = 'missing' }\n\
         console.log(`${requireFromProject('./package.json').name}: ${plugin}`)\n",
    )
    .expect("write tool/bin.js");
}

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

#[test]
fn development_preinstall_hooks_give_installed_tools_their_custom_modules_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    append_workspace_yaml_key(&workspace, "preferSymlinkedExecutables", "true");
    append_workspace_yaml_key(&workspace, "hoistPattern", "[]");
    write_probe_tool(&workspace);
    write_plugin(&workspace);
    let mut manifest = serde_json::json!({
        "name": "root",
        "dependencies": { "plugin": "file:plugin", "tool": "file:tool" },
    });
    write_manifest(&workspace, &manifest);
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    manifest["scripts"] = serde_json::json!({
        "pnpm:devPreinstall": "tool > preinstall-output.txt",
    });
    write_manifest(&workspace, &manifest);
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let output = fs::read_to_string(workspace.join("preinstall-output.txt"))
        .expect("read preinstall-output.txt");
    assert_eq!(output.trim_end(), "root: plugin loaded");
    drop((root, mock_instance));
}

#[test]
fn the_hoisted_linker_installs_into_a_custom_modules_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "deps/vendor");
    append_workspace_yaml_key(&workspace, "nodeLinker", "hoisted");
    append_workspace_yaml_key(&workspace, "packages", "['project-1']");
    write_manifest(
        &workspace,
        &serde_json::json!({ "name": "root", "dependencies": { "is-positive": "3.1.0" } }),
    );
    write_manifest(
        &workspace.join("project-1"),
        &serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        }),
    );
    let installed_version = |dir: &Path| -> String {
        let manifest = fs::read_to_string(dir.join("is-positive/package.json"))
            .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
        let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("parse manifest");
        manifest["version"]
            .as_str()
            .expect("version")
            .to_string()
    };

    for args in [&["install"][..], &["install", "--frozen-lockfile"]] {
        pacquet_in(&workspace)
            .with_args(args)
            .assert()
            .success();
        assert_eq!(installed_version(&workspace.join("deps/vendor")), "3.1.0", "{args:?}");
        assert_eq!(
            installed_version(&workspace.join("project-1/deps/vendor")),
            "1.0.0",
            "{args:?}",
        );
        assert!(!workspace.join("project-1/node_modules").exists(), "{args:?}");
        assert!(!workspace.join("node_modules/is-positive").exists(), "{args:?}");
        assert!(workspace.join("deps/vendor/.pnpm/lock.yaml").is_file(), "{args:?}");
        assert!(!workspace.join("vendor").exists(), "{args:?}");
        fs::remove_dir_all(workspace.join("deps/vendor")).expect("remove deps/vendor");
        fs::remove_dir_all(workspace.join("project-1/deps/vendor"))
            .expect("remove project-1/deps/vendor");
    }

    drop((root, mock_instance));
}

/// Changing `extendNodePath` changes neither the lockfile nor
/// `.modules.yaml`, so only the recorded setting tells the repeat install
/// that the shims are stale.
#[test]
fn repeat_install_rewrites_registry_bins_when_extend_node_path_changes() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "vendor");
    append_workspace_yaml_key(&workspace, "extendNodePath", "false");
    write_manifest(
        &workspace,
        &serde_json::json!({ "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" } }),
    );
    let install = |env_value: Option<&str>| {
        let mut command = pacquet_in(&workspace).with_arg("install");
        if let Some(value) = env_value {
            command = command.with_env("PNPM_CONFIG_EXTEND_NODE_PATH", value);
        }
        command.assert().success();
    };

    install(None);
    assert!(!registry_bin_sets_node_path(&workspace));

    set_extend_node_path(&workspace, "false", "true");
    pnpm_testing_utils::fs::bump_mtime(&workspace.join("pnpm-workspace.yaml"));
    install(None);
    assert!(registry_bin_sets_node_path(&workspace));

    install(Some("false"));
    assert!(!registry_bin_sets_node_path(&workspace));

    install(Some("true"));
    assert!(registry_bin_sets_node_path(&workspace));

    drop((root, mock_instance));
}

fn registry_bin_sets_node_path(project_dir: &Path) -> bool {
    let shim = if cfg!(windows) {
        "vendor/.bin/hello-world-js-bin.cmd"
    } else {
        "vendor/.bin/hello-world-js-bin"
    };
    fs::read_to_string(project_dir.join(shim)).expect("read the shim").contains("NODE_PATH=")
}

#[test]
fn a_modules_dir_outside_the_project_is_the_root_projects_only() {
    for linker in ["isolated", "hoisted"] {
        for setting in [ModulesDirSetting::Absolute, ModulesDirSetting::Climbing] {
            install_with_modules_dir_outside_the_project(linker, setting);
        }
    }
}

#[derive(Clone, Copy)]
enum IsPositive {
    Declared,
    Removed,
}

#[derive(Debug, Clone, Copy)]
enum ModulesDirSetting {
    Absolute,
    Climbing,
}

fn install_with_modules_dir_outside_the_project(linker: &str, setting: ModulesDirSetting) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (root_modules_dir, project_modules_dir) =
        set_modules_dir_outside_the_project(&workspace, setting);
    append_workspace_yaml_key(&workspace, "nodeLinker", linker);
    append_workspace_yaml_key(&workspace, "packages", "['project-1']");
    append_workspace_yaml_key(&workspace, "allowBuilds", "{'@pnpm.e2e/generated-bins': true}");
    let write_manifests = |is_positive: IsPositive| {
        let dependencies = |version: &str| {
            let mut dependencies = serde_json::json!({ "@pnpm.e2e/hello-world-js-bin": "1.0.0" });
            match is_positive {
                IsPositive::Declared => dependencies["is-positive"] = version.into(),
                IsPositive::Removed => {}
            }
            dependencies
        };
        let mut root_dependencies = dependencies("3.1.0");
        root_dependencies["@pnpm.e2e/generated-bins"] = "1.0.0".into();
        write_manifest(
            &workspace,
            &serde_json::json!({
                "name": "root",
                "scripts": { "postinstall": "hello-world-js-bin" },
                "dependencies": root_dependencies,
            }),
        );
        write_manifest(
            &workspace.join("project-1"),
            &serde_json::json!({
                "name": "project-1",
                "version": "1.0.0",
                "dependencies": dependencies("1.0.0"),
            }),
        );
    };
    let installed_version = |dir: &Path| -> String {
        let manifest = fs::read_to_string(dir.join("is-positive/package.json"))
            .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
        let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("parse manifest");
        manifest["version"]
            .as_str()
            .expect("version")
            .to_string()
    };
    let assert_nothing_outside = |context: &str| {
        assert!(!workspace.join("vendor").exists(), "{context}");
        assert!(!root_modules_dir.join("../project-1").exists(), "{context}");
    };
    write_manifests(IsPositive::Declared);

    for args in [&["install"][..], &["install", "--frozen-lockfile"]] {
        let context = format!("{linker} {setting:?} {args:?}");
        pacquet_in(&workspace)
            .with_args(args)
            .assert()
            .success();
        assert_eq!(installed_version(&root_modules_dir), "3.1.0", "{context}");
        assert_eq!(installed_version(&project_modules_dir), "1.0.0", "{context}");
        assert!(has_bin(&root_modules_dir, "hello-world-js-bin"), "{context}");
        assert!(has_bin(&root_modules_dir, "cmd1"), "{context}");
        assert_nothing_outside(&context);
        fs::remove_dir_all(&root_modules_dir).expect("remove the root modules dir");
        fs::remove_dir_all(&project_modules_dir).expect("remove project-1/vendor");
    }

    let context = format!("{linker} {setting:?} removal");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    write_manifests(IsPositive::Removed);
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!root_modules_dir.join("is-positive").exists(), "{context}");
    assert!(!project_modules_dir.join("is-positive").exists(), "{context}");
    assert!(has_bin(&root_modules_dir, "hello-world-js-bin"), "{context}");
    assert_nothing_outside(&context);

    drop((root, mock_instance));
}

fn has_bin(modules_dir: &Path, command: &str) -> bool {
    fs::read_dir(modules_dir.join(".bin"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(command)
        })
}

/// The root's and project-1's modules directories for `setting`, after
/// writing it to `pnpm-workspace.yaml`.
fn set_modules_dir_outside_the_project(
    workspace: &Path,
    setting: ModulesDirSetting,
) -> (PathBuf, PathBuf) {
    let root_modules_dir = workspace
        .parent()
        .expect("workspace parent")
        .join("vendor");
    let modules_dir = match setting {
        ModulesDirSetting::Absolute => root_modules_dir.to_string_lossy().replace('\\', "/"),
        ModulesDirSetting::Climbing => "../vendor".to_string(),
    };
    append_workspace_yaml_key(workspace, "modulesDir", &modules_dir);
    (root_modules_dir, workspace.join("project-1/vendor"))
}

#[test]
fn links_excluded_from_the_lockfile_go_to_a_modules_dir_outside_the_project() {
    for linker in ["isolated", "hoisted"] {
        for setting in [ModulesDirSetting::Absolute, ModulesDirSetting::Climbing] {
            link_with_modules_dir_outside_the_project(linker, setting);
        }
    }
}

fn link_with_modules_dir_outside_the_project(linker: &str, setting: ModulesDirSetting) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (root_modules_dir, project_modules_dir) =
        set_modules_dir_outside_the_project(&workspace, setting);
    append_workspace_yaml_key(&workspace, "nodeLinker", linker);
    append_workspace_yaml_key(&workspace, "packages", "['project-1']");
    append_workspace_yaml_key(&workspace, "excludeLinksFromLockfile", true);
    fs::create_dir_all(workspace.join("linked")).expect("create the linked package");
    fs::write(workspace.join("linked/bin.js"), "#!/usr/bin/env node\n").expect("write bin.js");
    write_manifest(
        &workspace.join("linked"),
        &serde_json::json!({ "name": "linked", "version": "1.0.0", "bin": { "linked-bin": "bin.js" } }),
    );
    write_manifest(
        &workspace,
        &serde_json::json!({ "name": "root", "devDependencies": { "linked": "link:./linked" } }),
    );
    write_manifest(
        &workspace.join("project-1"),
        &serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "devDependencies": { "linked": "link:../linked" },
        }),
    );

    for args in [&["install"][..], &["install", "--frozen-lockfile"]] {
        let context = format!("{linker} {setting:?} {args:?}");
        pacquet_in(&workspace)
            .with_args(args)
            .assert()
            .success();
        assert!(root_modules_dir.join("linked/package.json").exists(), "{context}");
        assert!(has_bin(&root_modules_dir, "linked-bin"), "{context}");
        assert!(project_modules_dir.join("linked/package.json").exists(), "{context}");
        assert!(!workspace.join("vendor").exists(), "{context}");
    }

    let context = format!("{linker} {setting:?} --prod");
    pacquet_in(&workspace)
        .with_args(["install", "--prod"])
        .assert()
        .success();
    assert!(!root_modules_dir.join("linked").exists(), "{context}");
    assert!(!project_modules_dir.join("linked").exists(), "{context}");

    drop((root, mock_instance));
}

#[test]
fn the_loaded_linker_links_bins_into_a_modules_dir_outside_the_project() {
    for setting in [ModulesDirSetting::Absolute, ModulesDirSetting::Climbing] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        let (root_modules_dir, _) = set_modules_dir_outside_the_project(&workspace, setting);
        append_workspace_yaml_key(&workspace, "nodeLinker", "{type: loaded}");
        append_workspace_yaml_key(&workspace, "allowBuilds", "{'@pnpm.e2e/generated-bins': true}");
        write_manifest(
            &workspace,
            &serde_json::json!({
                "name": "root",
                "dependencies": {
                    "@pnpm.e2e/hello-world-js-bin": "1.0.0",
                    "@pnpm.e2e/generated-bins": "1.0.0",
                },
            }),
        );

        pacquet_in(&workspace)
            .with_arg("install")
            .assert()
            .success();
        assert!(has_bin(&root_modules_dir, "hello-world-js-bin"), "{setting:?}");
        assert!(has_bin(&root_modules_dir, "cmd1"), "{setting:?}");
        assert!(!workspace.join("vendor").exists(), "{setting:?}");

        drop((root, mock_instance));
    }
}
